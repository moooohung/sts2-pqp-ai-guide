//! Necrobinder card ports from Models/Cards (C#).
use super::kit::*;
use crate::card::{CId, KRef};
use crate::combat::Combat;
use crate::combat::{PileKind, PilePos, Props};
use crate::choice::{ChoiceSrc, Resolver};
use crate::tables::*;

pub(crate) fn play(c: &mut Combat, k: KRef, t: Option<CId>) -> bool {
    match c.cards[k as usize].id {
        // CallOfTheVoid.OnPlay: canonical amount is one.
        CardId::CallOfTheVoid => { self_power(c, k, PowerId::CallOfTheVoid, 1); }
        // Oblivion.OnPlay: the mark is attributed to the card owner, not the target.
        CardId::Oblivion => {
            c.apply_power_to(target(t), PowerId::Oblivion, var(c, k, VarKey::Doom, 3), Some(owner(c, k)));
        }
        // Models/Cards/{SentryMode,SpiritOfAsh}.cs OnPlay.
        CardId::SentryMode => {
            c.apply_power_to(owner(c, k), PowerId::SentryMode, var(c, k, VarKey::SentryMode, 1), Some(owner(c, k)));
        }
        CardId::SpiritOfAsh => {
            c.apply_power_to(owner(c, k), PowerId::SpiritOfAsh, var(c, k, VarKey::BlockOnExhaust, 4), Some(owner(c, k)));
        }
        // SweepingGaze.OnPlay: no Osty means no attack or target RNG.
        CardId::SweepingGaze => {
            c.osty_card_hit_random(k, var(c, k, VarKey::OstyDamage, 10));
        }
        // Bury.OnPlay / Reanimate.OnPlay: canonical factory supplies upgrades.
        CardId::Bury => { hit(c, k, target(t)); }
        CardId::Reanimate => { c.summon_osty(owner(c, k), var(c, k, VarKey::Summon, 20)); }
        // EndOfDays.OnPlay snapshots HittableEnemies for application, then obtains
        // a fresh snapshot for DoomPower.GetDoomedCreatures/DoomKill.
        CardId::EndOfDays => {
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Doom, var(c, k, VarKey::Doom, 29), Some(owner(c, k)));
            }
            let doomed: Vec<_> = c.hittable_enemies().into_iter().filter(|&e|
                c.has_power(e, PowerId::Doom) && c.cr(e).hp <= c.power_amount(e, PowerId::Doom)).collect();
            c.doom_kill(&doomed);
        }
        // Invoke.OnPlay: independently stacked next-turn summon and energy.
        CardId::Invoke => {
            let who = owner(c, k);
            c.apply_power_to(who, PowerId::SummonNextTurn, var(c, k, VarKey::Summon, 2), Some(who));
            c.apply_power_to(who, PowerId::EnergyNextTurn, var(c, k, VarKey::Energy, 2), Some(who));
        }
        // DanseMacabre.OnPlay; the power's canonical threshold is two energy.
        CardId::DanseMacabre => {
            c.apply_power_to(owner(c, k), PowerId::DanseMacabre, var(c, k, VarKey::DanseMacabre, 4), Some(owner(c, k)));
        }
        // Severance.OnPlay: materialize all three BASE Souls before any generation
        // callbacks; then Random draw, bottom discard, hand in that order.
        CardId::Severance => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            let who = owner(c, k);
            let souls: Vec<_> = (0..3).filter_map(|_| c.new_card(who, CardId::Soul, false)).collect();
            for (q, pile) in souls.into_iter().zip([PileKind::Draw, PileKind::Discard, PileKind::Hand]) {
                let pos = if pile == PileKind::Draw { PilePos::Random } else { PilePos::Bottom };
                c.add_generated_card(q, pile, pos, true);
            }
        }
        // Squeeze.CalculatedDamage counts other owned OstyAttack cards in ALL
        // combat piles (including Exhaust/Play), not distinct card names.
        CardId::Squeeze => {
            let who = owner(c, k);
            let count = c.pile_cards(who).into_iter().filter(|&q|
                q != k && c.cards[q as usize].has_tag(CardTag::OstyAttack)).count() as i32;
            let amount = var(c, k, VarKey::CalcBase, 25)
                + var(c, k, VarKey::ExtraDamage, 5) * count;
            c.osty_card_hit(k, target(t), amount);
        }
        // TimesUp.CalculatedDamage uses target Doom without consuming it;
        // the attack still runs when Doom is zero (Strength can add damage).
        CardId::TimesUp => {
            let tt = target(t);
            let amount = var(c, k, VarKey::CalcBase, 0)
                + var(c, k, VarKey::ExtraDamage, 1) * c.power_amount(tt, PowerId::Doom);
            hit_base(c, k, tt, amount);
        }
        // Neurosurge.OnPlay: energy, then draw, THEN the recurring Doom power.
        CardId::Neurosurge => {
            let who = owner(c, k);
            gain_energy(c, who, var(c, k, VarKey::Energy, 3));
            draw(c, k, var(c, k, VarKey::Cards, 2));
            if c.pending_choice.is_some() {
                c.unported("Neurosurge draw choice continuation");
            } else {
                c.apply_power_to(who, PowerId::Neurosurge, var(c, k, VarKey::Neurosurge, 3), Some(who));
            }
        }
        // Undeath.OnPlay / CardModel.CreateClone: clone the actual combat instance,
        // retaining local keywords, costs and dynamic vars, not a factory template.
        CardId::Undeath => {
            block(c, k);
            let iid = c.run.new_card_id();
            let clone = c.cards[k as usize].clone_with_id(iid);
            let q = c.cards.len() as KRef;
            c.cards.push(clone);
            c.add_generated_card(q, PileKind::Discard, PilePos::Bottom, true);
        }
        // LegionOfBone.OnPlay snapshots ALL living player creatures, caster included.
        CardId::LegionOfBone => {
            for who in c.living_players() {
                c.summon_osty(who, var(c, k, VarKey::Summon, 6));
            }
        }
        // Models/Cards/Underworld.cs OnPlay; upgrade removes Exhaust.
        CardId::Underworld => {
            c.apply_power_to(owner(c, k), PowerId::Underworld, 1, Some(owner(c, k)));
        }
        // Models/Cards/{Cacophony,SleightOfFlesh,EnfeeblingTouch,Friendship}.cs OnPlay.
        CardId::Cacophony => {
            c.apply_power_to(owner(c, k), PowerId::Cacophony, var(c, k, VarKey::Damage, 66), Some(owner(c, k)));
        }
        CardId::SleightOfFlesh => {
            c.apply_power_to(owner(c, k), PowerId::SleightOfFlesh, var(c, k, VarKey::SleightOfFlesh, 9), Some(owner(c, k)));
        }
        CardId::EnfeeblingTouch => {
            c.apply_power_to(target(t), PowerId::EnfeeblingTouch, var(c, k, VarKey::StrengthLoss, 8), Some(owner(c, k)));
        }
        CardId::Friendship => {
            let who = owner(c, k);
            c.apply_power_to(who, PowerId::Strength, -var(c, k, VarKey::Strength, 2), Some(who));
            c.apply_power_to(who, PowerId::Friendship, var(c, k, VarKey::Energy, 1), Some(who));
        }
        // Models/Cards/NecroMastery.cs OnPlay: summon completes before applying power.
        CardId::NecroMasteryCard => {
            let who = owner(c, k);
            c.summon_osty(who, var(c, k, VarKey::Summon, 5));
            c.apply_power_to(who, PowerId::NecroMastery, 1, Some(who));
        }
        // Models/Cards/{Shroud,Haunt,Defy}.cs OnPlay.
        CardId::Shroud => {
            c.apply_power_to(owner(c, k), PowerId::Shroud, var(c, k, VarKey::Block, 3), Some(owner(c, k)));
        }
        CardId::Haunt => {
            c.apply_power_to(owner(c, k), PowerId::Haunt, var(c, k, VarKey::HpLoss, 7), Some(owner(c, k)));
        }
        CardId::Defy => {
            block(c, k);
            c.apply_power_to(target(t), PowerId::Weak, var(c, k, VarKey::Weak, 1), Some(owner(c, k)));
        }
        // Models/Cards/StrikeNecrobinder.cs OnPlay; canonical upgrade is +3 damage.
        CardId::StrikeNecrobinder => { hit(c, k, target(t)); }
        // Models/Cards/Soulbound.cs OnPlay: target owns the power, caster is Applier.
        CardId::Soulbound => {
            c.apply_power_to(target(t), PowerId::Soulbound, 1, Some(owner(c, k)));
        }
        // Models/Cards/Calcify.cs OnPlay.
        CardId::CalcifyCard => self_power(c, k, PowerId::Calcify, var(c, k, VarKey::Calcify, 4)),
        // Models/Cards/Seance.cs OnPlay: draw-pile selector, not a random transform.
        CardId::Seance => {
            let mut cards = c.players[owner(c, k)].draw.to_vec();
            super::cards_colorless::sort_draw_selector(&c.cards, &mut cards);
            let count = var(c, k, VarKey::Cards, 1).max(0) as u16;
            c.request_multi_card_choice(&cards, PileKind::Draw, Resolver {
                src: ChoiceSrc::Card(CardId::Seance), k, target: None, step: 0, arg: 0,
            }, count, Some(count), false);
        }
        // ForbiddenGrimoire.OnPlay: one Counter-stacked power on the card owner.
        CardId::ForbiddenGrimoire => self_power(c, k, PowerId::ForbiddenGrimoire, 1),
        // Models/Cards/NoEscape.cs CalculatedDoom: base + extra * floor(Doom / threshold).
        CardId::NoEscape => {
            let tt = target(t);
            let threshold = var(c, k, VarKey::DoomThreshold, 10);
            let amount = var(c, k, VarKey::CalcBase, 10)
                + var(c, k, VarKey::CalcExtra, 5) * c.power_amount(tt, PowerId::Doom).div_euclid(threshold);
            c.apply_power_to(tt, PowerId::Doom, amount, Some(owner(c, k)));
        }
        // Models/Cards/Snap.cs OnPlay still selects Retain when Osty is missing.
        CardId::Snap => {
            c.osty_card_hit(k, target(t), var(c, k, VarKey::OstyDamage, 7));
            let cards: Vec<_> = c.players[owner(c, k)].hand.iter().copied()
                .filter(|&q| q != k && !c.cards[q as usize].is_retain()).collect();
            c.request_card_choice(&cards, PileKind::Hand, Resolver {
                src: ChoiceSrc::Card(CardId::Snap), k, target: None, step: 0, arg: 0,
            }, false);
        }
        // Models/Cards/Bodyguard.cs OnPlay.
        CardId::Bodyguard => { c.summon_osty(owner(c, k), var(c, k, VarKey::Summon, 5)); }
        // Models/Cards/GraveWarden.cs OnPlay gains block before random Soul insertion.
        CardId::GraveWarden => {
            block(c, k);
            souls_in_draw(c, owner(c, k), var(c, k, VarKey::Cards, 1), false);
        }
        // Models/Cards/DrainPower.cs OnPlay / IEnumerableExtensions.TakeRandom:
        // snapshot upgradable discard cards, shuffle in current pile order, then upgrade.
        CardId::DrainPower => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            let mut cards = Vec::new();
            for q in c.players[owner(c, k)].discard.clone() {
                if c.cards[q as usize].upgraded { continue; }
                // Preserve the existing r7 identity projection used by StoneCracker:
                // its IsUpgradable template consumes an ID, not a combat card.
                c.run.new_card_id();
                if card_info(c.cards[q as usize].id, true).is_some_and(|u| u.upgraded) {
                    cards.push(q);
                }
            }
            // DrainPower.OnPlay always enumerates TakeRandom, even on a lethal hit.
            // CardCmd.Upgrade alone refuses mutation while combat is ending.
            c.run.rngs.combat_card_selection.shuffle(&mut cards);
            for q in cards.into_iter().take(var(c, k, VarKey::Cards, 2).max(0) as usize) {
                if !c.is_over_or_ending() {
                    c.upgrade_card(q);
                }
            }
        }
        // Models/Cards/Demesne.cs / DevourLife.cs OnPlay.
        CardId::Demesne => self_power(c, k, PowerId::Demesne, var(c, k, VarKey::Cards, 1)),
        CardId::DevourLifeCard => self_power(c, k, PowerId::DevourLife, var(c, k, VarKey::DevourLife, 1)),
        // Models/Cards/BoneShards.cs OnPlay: missing Osty skips all three effects.
        // A single all-enemy pet attack completes before player block, then pet death.
        CardId::BoneShards => {
            let who = owner(c, k);
            if c.osty(who).is_some_and(|o| c.cr(o).is_alive()) {
                c.osty_card_hit_all(k, var(c, k, VarKey::OstyDamage, 9));
                block(c, k);
                if let Some(o) = c.osty(who).filter(|&o| c.cr(o).is_alive()) {
                    c.kill_creature(o);
                }
            }
        }
        // Models/Cards/Veilpiercer.cs OnPlay.
        CardId::Veilpiercer => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            self_power(c, k, PowerId::Veilpiercer, 1);
        }
        // Models/Cards/Transfigure.cs OnPlay excludes itself from the hand selection.
        CardId::Transfigure => {
            let cards: Vec<_> = c.players[owner(c, k)].hand.iter().copied().filter(|&q| q != k).collect();
            c.request_card_choice(&cards, PileKind::Hand, Resolver {
                src: ChoiceSrc::Card(CardId::Transfigure), k, target: None, step: 0, arg: 0,
            }, false);
        }
        // Models/Cards/BorrowedTime.cs OnPlay: gain energy before applying the surcharge.
        CardId::BorrowedTime => {
            gain_energy(c, owner(c, k), var(c, k, VarKey::Energy, 4));
            self_power(c, k, PowerId::BorrowedTime, var(c, k, VarKey::ExtraCost, 1));
        }
        // Models/Cards/DeathMarch.cs CalculatedDamage counts only non-hand draws
        // attributed to this card's owner, including nested extra draws.
        CardId::DeathMarch => {
            let count = c.hist.extra_draws_this_turn[owner(c, k) as usize];
            let damage = var(c, k, VarKey::CalcBase, 8) + var(c, k, VarKey::ExtraDamage, 4) * count;
            hit_base(c, k, target(t), damage);
        }
        // Models/Cards/Cleanse.cs OnPlay: summon before choosing a draw-pile exhaust.
        CardId::Cleanse => {
            let who = owner(c, k);
            c.summon_osty(who, var(c, k, VarKey::Summon, 3));
            let mut cards = c.players[who].draw.to_vec();
            super::cards_colorless::sort_draw_selector(&c.cards, &mut cards);
            c.request_card_choice(&cards, PileKind::Draw, Resolver {
                src: ChoiceSrc::Card(CardId::Cleanse), k, target: None, step: 0, arg: 0,
            }, false);
        }
        // Models/Cards/Defile.cs OnPlay.
        CardId::Defile => { hit(c, k, target(t)); }
        // Models/Cards/Countdown.cs OnPlay.
        CardId::CountdownCard => self_power(c, k, PowerId::Countdown, var(c, k, VarKey::Countdown, 6)),
        // Models/Cards/Delay.cs OnPlay: gain block before next-turn energy.
        CardId::Delay => {
            block(c, k);
            self_power(c, k, PowerId::EnergyNextTurn, var(c, k, VarKey::Energy, 1));
        }
        // Models/CardPools/NecrobinderCardPool.GenerateAllCards includes PullAggro.
        // Models/Cards/PullAggro.cs OnPlay: summon first, then gain player block.
        CardId::PullAggro => {
            c.summon_osty(owner(c, k), var(c, k, VarKey::Summon, 4));
            block(c, k);
        }
        // TheScythe.OnPlay buffs this combat copy AND its owning player's DeckVersion.
        CardId::TheScythe => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            let amount = var(c, k, VarKey::Increase, 5);
            let who = owner(c, k);
            let deck = c.cards[k as usize].deck_version;
            grow_scythe(&mut c.cards[k as usize], amount);
            if let Some(index) = deck {
                if let Some(original) = c.player_deck_mut(who).get_mut(index as usize)
                    && original.id == CardId::TheScythe
                {
                    original.cv.scythe_growth = original.damage.unwrap_or(13) - 13;
                    grow_scythe(original, amount);
                } else {
                    c.unported("Scythe missing deck version");
                }
            }
        }
        // BlightStrike.OnPlay: Doom uses the immutable DamageResult.TotalDamage, not overkill.
        CardId::BlightStrike => {
            let who = owner(c, k);
            let tt = target(t);
            let damage = c.calculate_damage(c.cards[k as usize].damage.unwrap_or(8), Some(who), tt, Props::MOVE);
            let result = c.apply_damage_result(tt, damage, Props::MOVE, Some(who));
            c.flush_pending_attack();
            c.apply_power_to(tt, PowerId::Doom, result.total(), Some(who));
        }
        // Afterlife.OnPlay.
        CardId::Afterlife => {
            c.summon_osty(owner(c, k), var(c, k, VarKey::Summon, 6));
        }
        // CaptureSpirit.OnPlay calls CreatureCmd.Damage, NOT AttackCommand.
        CardId::CaptureSpirit => {
            let who = owner(c, k);
            let tt = target(t);
            let props = Props::MOVE.or(Props::UNPOWERED).or(Props::UNBLOCKABLE);
            let damage = c.modify_damage(var(c, k, VarKey::Damage, 3), Some(who), tt, props, Some(k));
            c.apply_damage(tt, damage, props, Some(who));
            souls_in_draw(c, who, var(c, k, VarKey::Cards, 3), false);
        }
        // Dirge.OnPlay resolves X once; separate summons fire their hooks individually.
        CardId::Dirge => {
            let who = owner(c, k);
            let x = c.cards[k as usize].energy_spent;
            for _ in 0..x {
                c.summon_osty(who, var(c, k, VarKey::Summon, 3));
            }
            souls_in_draw(c, who, x, c.cards[k as usize].upgraded);
        }
        // Scourge.OnPlay.
        CardId::Scourge => {
            c.apply_power_to(target(t), PowerId::Doom, var(c, k, VarKey::Doom, 13), Some(owner(c, k)));
            draw(c, k, var(c, k, VarKey::Cards, 1));
        }
        // Models/Cards/Debilitate.cs OnPlay completes its attack before applying the mark.
        CardId::DebilitateCard => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            c.apply_power_to(target(t), PowerId::Debilitate, var(c, k, VarKey::Debilitate, 2), Some(owner(c, k)));
        }
        // Models/Cards/Eidolon.cs OnPlay snapshots eligible exhausted cards before autoplay.
        CardId::Eidolon => {
            let cards: Vec<_> = c.players[owner(c, k)].exhaust.iter().copied()
                .filter(|&q| c.cards[q as usize].is_ethereal() && !c.cards[q as usize].is_unplayable()).collect();
            c.autoplay_snapshot(cards);
        }
        // Models/Cards/Pagestorm.cs OnPlay.
        CardId::Pagestorm => self_power(c, k, PowerId::Pagestorm, var(c, k, VarKey::Cards, 1)),
        // Models/Cards/SharedFate.cs OnPlay: owner's Strength loss precedes the enemy's.
        CardId::SharedFate => {
            let who = owner(c, k);
            c.apply_power_to(who, PowerId::Strength, -var(c, k, VarKey::PlayerStrengthLoss, 2), Some(who));
            c.apply_power_to(target(t), PowerId::Strength, -var(c, k, VarKey::EnemyStrengthLoss, 2), Some(who));
        }
        // Models/Cards/Deathbringer.cs OnPlay: all Doom applications precede all Weak.
        CardId::Deathbringer => {
            let who = owner(c, k);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Doom, var(c, k, VarKey::Doom, 21), Some(who));
            }
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Weak, var(c, k, VarKey::Weak, 1), Some(who));
            }
        }
        // Spur.OnPlay heals AFTER Summon has grown/healed or revived this owner's Osty.
        CardId::Spur => {
            if let Some(o) = c.summon_osty(owner(c, k), var(c, k, VarKey::Summon, 3)) {
                c.heal(o, var(c, k, VarKey::Heal, 5));
            }
        }
        // DeathsDoor.OnPlay: each gain is separately modified (Dexterity/Frail, block hooks).
        CardId::DeathsDoor => {
            let who = owner(c, k);
            let repeats = if c.hist.doom_appliers_this_turn[who as usize] {
                1 + var(c, k, VarKey::Repeat, 2)
            } else { 1 };
            for _ in 0..repeats { block(c, k); }
        }
        // NegativePulse.OnPlay gains block before applying Doom to each hittable enemy.
        CardId::NegativePulse => {
            block(c, k);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Doom, var(c, k, VarKey::Doom, 7), Some(owner(c, k)));
            }
        }
        // Dredge.OnPlay / CardSelectorPrefs(prompt, count): select exactly count, capped
        // by available cards and remaining hand capacity; empty/full cases have no prompt.
        CardId::Dredge => {
            let who = owner(c, k);
            let cards = c.players[who].discard.clone();
            let count = (var(c, k, VarKey::Cards, 3).max(0) as usize)
                .min(crate::combat::MAX_HAND_SIZE.saturating_sub(c.players[who].hand.len()))
                .min(cards.len());
            if count > 0 {
                c.request_multi_card_choice(&cards, PileKind::Discard, Resolver {
                    src: ChoiceSrc::Card(CardId::Dredge), k, target: None, step: 0, arg: 0,
                }, count as u16, Some(count as u16), false);
            }
        }
        // Lethality.OnPlay / ReaperForm.OnPlay.
        CardId::LethalityCard => self_power(c, k, PowerId::Lethality, var(c, k, VarKey::Lethality, 50)),
        CardId::ReaperForm => self_power(c, k, PowerId::ReaperForm, 1),
        // Wisp.OnPlay.
        CardId::Wisp => gain_energy(c, owner(c, k), var(c, k, VarKey::Energy, 1)),
        // SculptingStrike.OnPlay excludes itself and cards with LOCAL Ethereal.
        CardId::SculptingStrike => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            // CardSelectCmd.FromHand returns an empty selection once combat is
            // over or ending (C# CardSelectCmd.cs:657-662).
            if c.is_over_or_ending() { return true; }
            let cards: Vec<_> = c.players[owner(c, k)].hand.iter().copied().filter(|&q|
                q != k && (!c.cards[q as usize].is_ethereal()
                    || c.cards[q as usize].cv.affliction_added_ethereal)).collect();
            if !cards.is_empty() {
                c.request_card_choice(&cards, PileKind::Hand, Resolver {
                    src: ChoiceSrc::Card(CardId::SculptingStrike), k, target: None, step: 0, arg: 0,
                }, false);
            }
        }
        // Models/Cards/Rattle.cs CalculatedHits counts prior CreatureAttackedEntry actors,
        // not individual hits or targets; resolve before starting this attack.
        CardId::Rattle => {
            if let Some(o) = c.osty(owner(c, k)).filter(|&o| c.cr(o).is_alive()) {
                let count = c.hist.attacked_this_turn.iter().filter(|&&actor| actor == o).count() as i32;
                let hits = var(c, k, VarKey::CalcBase, 0) + var(c, k, VarKey::CalcExtra, 1) * (1 + count);
                c.osty_card_hit_n(k, target(t), var(c, k, VarKey::OstyDamage, 7), hits);
            }
        }
        // Models/Cards/Fetch.cs OnPlay: draw only after a living Osty's attack and
        // only before this particular card's first finished play of the side turn.
        CardId::Fetch => {
            if c.osty(owner(c, k)).is_some_and(|o| c.cr(o).is_alive()) {
                c.osty_card_hit(k, target(t), var(c, k, VarKey::OstyDamage, 3));
                if !c.hist.played_this_turn.iter().any(|&(_, played)| played == k) {
                    draw(c, k, var(c, k, VarKey::Cards, 1));
                }
            }
        }
        // Melancholy.OnPlay / DefendNecrobinder.OnPlay.
        CardId::Melancholy | CardId::DefendNecrobinder => { block(c, k); }
        // RightHandHand.OnPlay / Flatten.OnPlay: missing Osty is a no-op.
        CardId::RightHandHand | CardId::Flatten | CardId::Poke => {
            let damage = c.cards[k as usize].var(VarKey::OstyDamage).expect("OstyDamage var");
            c.osty_card_hit(k, target(t), damage);
        }
        // HighFive.OnPlay: one all-enemy Osty attack, then Vulnerable on survivors.
        CardId::HighFive => {
            let who = owner(c, k);
            if c.osty(who).is_some_and(|o| c.cr(o).is_alive()) {
                c.osty_card_hit_all(k, var(c, k, VarKey::OstyDamage, 11));
                for e in c.hittable_enemies() {
                    c.apply_power_to(e, PowerId::Vulnerable, var(c, k, VarKey::Vulnerable, 2), Some(who));
                }
            }
        }
        // BansheesCry.OnPlay.
        CardId::BansheesCry => hit_all(c, k),
        // SicEm.OnPlay applies the mark even if Osty is missing.
        CardId::SicEm => {
            let tt = target(t);
            let damage = var(c, k, VarKey::OstyDamage, 5);
            c.osty_card_hit(k, tt, damage);
            c.apply_power_to(tt, PowerId::SicEm, var(c, k, VarKey::SicEm, 3), Some(owner(c, k)));
        }
        // Unleash.OnPlay / CalculatedVar.Calculate: base + extra * Osty's current HP.
        CardId::Unleash => {
            let who = owner(c, k);
            if let Some(o) = c.osty(who).filter(|&o| c.cr(o).is_alive()) {
                let damage = var(c, k, VarKey::CalcBase, 6)
                    + var(c, k, VarKey::ExtraDamage, 1) * c.cr(o).hp;
                c.osty_card_hit(k, target(t), damage);
            }
        }
        // Fear.OnPlay: attack completes before applying Vulnerable.
        CardId::Fear => {
            let tt = target(t);
            hit(c, k, tt);
            c.flush_pending_attack();
            c.apply_power_to(tt, PowerId::Vulnerable, var(c, k, VarKey::Vulnerable, 1), Some(owner(c, k)));
        }
        // Hang.OnPlay: hit with the old multiplier, then double the mark (first = 2).
        CardId::Hang => {
            let tt = target(t);
            hit(c, k, tt);
            c.flush_pending_attack();
            let amount = c.power_amount(tt, PowerId::Hang);
            let increase = amount.max(2).min((999_999_999 - amount).max(0));
            c.apply_power_to(tt, PowerId::Hang, increase, Some(owner(c, k)));
        }
        // Eradicate.OnPlay: ResolveEnergyXValue includes the established X-cost modifiers.
        CardId::Eradicate => hit_n(c, k, target(t), c.cards[k as usize].energy_spent),
        // PullFromBelow.OnPlay counts finished Ethereal plays, not the current play.
        CardId::PullFromBelow => {
            let count = c.hist.ethereal_plays_finished[owner(c, k) as usize];
            let hits = var(c, k, VarKey::CalcBase, 0) + var(c, k, VarKey::CalcExtra, 1) * count;
            hit_n(c, k, target(t), hits);
        }
        // Sow.OnPlay.
        CardId::Sow => hit_all(c, k),
        // Sacrifice.OnPlay: calculate from max HP BEFORE killing Osty; block belongs to player.
        CardId::Sacrifice => {
            let who = owner(c, k);
            if let Some(o) = c.osty(who).filter(|&o| c.cr(o).is_alive()) {
                let amount = var(c, k, VarKey::CalcBase, 0)
                    + var(c, k, VarKey::CalcExtra, 1) * c.cr(o).max_hp * 3;
                c.kill_creature(o);
                block_base(c, k, amount);
            }
        }
        // GlimpseBeyond.OnPlay / CombatState.GetTeammatesOf includes the caster.
        CardId::GlimpseBeyond => {
            for who in c.living_players() {
                souls_in_draw_by(c, who, var(c, k, VarKey::Cards, 3), false, owner(c, k));
            }
        }
        // Soul.OnPlay / Models/Cards/Parse.cs OnPlay: owner's ordinary card draw.
        CardId::Soul => draw(c, k, var(c, k, VarKey::Cards, 2)),
        CardId::Parse => draw(c, k, var(c, k, VarKey::Cards, 3)),
        // Reap.OnPlay.
        CardId::Reap => { hit(c, k, target(t)); }
        // Graveblast.OnPlay: the attack finishes before the discard-pile choice.
        CardId::Graveblast => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            let cards = c.players[owner(c, k)].discard.clone();
            // CardSelectCmd.FromCombatPile returns empty without opening a prompt.
            if !cards.is_empty() {
                c.request_card_choice(&cards, PileKind::Discard, Resolver {
                    src: ChoiceSrc::Card(CardId::Graveblast), k, target: None, step: 0, arg: 0,
                }, false);
            }
        }
        CardId::Misery => misery(c, k, target(t)),
        // Putrefy.OnPlay: Weak first, then Vulnerable (Artifact consumes in that order).
        CardId::Putrefy => {
            let amount = var(c, k, VarKey::Power, 2);
            let who = owner(c, k);
            c.apply_power_to(target(t), PowerId::Weak, amount, Some(who));
            c.apply_power_to(target(t), PowerId::Vulnerable, amount, Some(who));
        }
        // Protector.OnPlay / CalculatedDamageVar: max HP, unlike Unleash's current HP.
        CardId::Protector => {
            if let Some(o) = c.osty(owner(c, k)).filter(|&o| c.cr(o).is_alive()) {
                let damage = var(c, k, VarKey::CalcBase, 10)
                    + var(c, k, VarKey::ExtraDamage, 1) * c.cr(o).max_hp;
                c.osty_card_hit(k, target(t), damage);
            }
        }
        // Reave.OnPlay: complete attack, then insert generated (upgraded when appropriate) Souls.
        CardId::Reave => {
            hit(c, k, target(t));
            c.flush_pending_attack();
            souls_in_draw(c, owner(c, k), var(c, k, VarKey::Cards, 1), c.cards[k as usize].upgraded);
        }
        // SoulStorm.OnPlay counts Souls currently in this owner's exhaust pile.
        CardId::SoulStorm => {
            let count = c.players[owner(c, k)].exhaust.iter()
                .filter(|&&q| c.cards[q as usize].id == CardId::Soul).count() as i32;
            let damage = var(c, k, VarKey::CalcBase, 9) + var(c, k, VarKey::ExtraDamage, 4) * count;
            hit_base(c, k, target(t), damage);
        }
        _ => return false,
    }
    true
}

/// Soul.Create materializes the complete batch before CardPileCmd.AddGeneratedCardsToCombat.
fn souls_in_draw(c: &mut Combat, who: CId, count: i32, upgraded: bool) {
    souls_in_draw_by(c, who, count, upgraded, who);
}

fn souls_in_draw_by(c: &mut Combat, who: CId, count: i32, upgraded: bool, creator: CId) {
    let souls: Vec<_> = (0..count).filter_map(|_| c.new_card(who, CardId::Soul, upgraded)).collect();
    for soul in souls {
        c.add_generated_card_by(soul, PileKind::Draw, PilePos::Random, Some(creator));
    }
}

/// TheScythe.BuffFromPlay / UpdateDamage: reset the base to 13 + saved growth.
fn grow_scythe(card: &mut crate::card::Card, amount: i32) {
    card.cv.scythe_growth += amount;
    let value = 13 + card.cv.scythe_growth;
    card.damage = Some(value);
    if let Some((_, damage)) = card.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
        *damage = value;
    }
}

/// Misery.OnPlay snapshots debuffs before the hit, preserving the original applier.
/// ClonePreservingMutability retains private/temporary state for fresh instances.
fn misery(c: &mut Combat, k: KRef, target: CId) {
    let mut debuffs: Vec<_> = c.cr(target).power_list().iter().filter_map(|&p| {
        let power = &c.powers[p as usize];
        (Combat::power_type_for_amount(power.id, power.amount) == PowerType::Debuff).then(|| power.clone())
    }).collect();
    // Misery.OnPlay compensates the captured underlying Strength for each
    // TemporaryStrengthPower before applying both copies. Otherwise the initial
    // BeforeApplied on the temporary copy would subtract the same loss twice.
    let compensation: i32 = debuffs.iter().filter(|p|
        Combat::is_temporary_strength_loss(p.id)).map(|p| p.amount).sum();
    if let Some(strength) = debuffs.iter_mut().find(|p| p.id == PowerId::Strength) {
        strength.amount += compensation;
    }
    hit(c, k, target);
    c.flush_pending_attack();
    for enemy in c.hittable_enemies() {
        if enemy == target { continue; }
        for power in &debuffs {
            if power.amount != 0 {
                c.apply_cloned_power(enemy, power, power.amount, k);
            }
        }
    }
}

/// Graveblast.OnPlay continuation.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    match r.src {
        ChoiceSrc::Card(CardId::Seance) => {
            // Seance.OnPlay calls TransformTo<Soul> separately for each selected card.
            // Each Transform finishes From/To, then AfterCardGeneratedForCombat(owner).
            for &q in selected {
                let who = owner(c, q);
                if !c.players[who].draw.contains(&q) {
                    c.unported("Seance selected card left draw pile");
                    break;
                }
                transform_in_pile(c, q, CardId::Soul, false);
            }
        }
        ChoiceSrc::Card(CardId::Snap) => {
            if let Some(&q) = selected.first() { c.cards[q as usize].keywords |= KW_RETAIN; }
        }
        ChoiceSrc::Card(CardId::Transfigure) => {
            for &q in selected {
                let card = &mut c.cards[q as usize];
                // GetWithModifiers(CostModifiers.None) reads the base, not local/global cost.
                if !card.x_cost && card.original_cost >= 0 {
                    card.add_combat_cost(1);
                }
                card.replay += 1;
            }
        }
        ChoiceSrc::Card(CardId::Cleanse) => {
            if let Some(&k) = selected.first() { exhaust(c, k); }
        }
        ChoiceSrc::Card(CardId::Graveblast | CardId::Dredge) => {
            c.move_cards_to_pile(selected, PileKind::Hand, PilePos::Bottom);
        }
        ChoiceSrc::Card(CardId::SculptingStrike) => {
            if let Some(&k) = selected.first() {
                c.cards[k as usize].keywords |= KW_ETHEREAL;
                // The keyword is now local; removing a global Hex must not erase it.
                c.cards[k as usize].cv.affliction_added_ethereal = false;
            }
        }
        _ => return false,
    }
    true
}

/// Flatten.ReduceCost: SetThisTurn(0), including clones entering after an Osty attack.
fn flatten_free(c: &mut Combat, k: KRef) {
    c.cards[k as usize].set_energy_cost(0, crate::card::CostExpiry::Turn, false);
}

/// Flatten.AfterAttack: only the card owner's Osty counts.
pub(crate) fn after_attack(c: &mut Combat, k: KRef, attacker: Option<CId>) {
    if c.cards[k as usize].id == CardId::Flatten
        && attacker.is_some() && attacker == c.osty(owner(c, k))
    {
        flatten_free(c, k);
    }
}

/// Flatten / BansheesCry.AfterCardEnteredCombat.
pub(crate) fn after_entered(c: &mut Combat, k: KRef, entered: KRef) {
    if k != entered {
        return;
    }
    let who = owner(c, k);
    match c.cards[k as usize].id {
        CardId::Flatten => {
            if c.osty(who).is_some_and(|o| c.hist.attacked_this_turn.contains(&o)) {
                flatten_free(c, k);
            }
        }
        CardId::BansheesCry if !c.cards[k as usize].cv.is_clone => {
            let discount = c.hist.ethereal_plays_finished[who as usize] * var(c, k, VarKey::Energy, 2);
            c.cards[k as usize].add_combat_cost(-discount);
        }
        _ => {}
    }
}

/// BansheesCry.AfterCardPlayed: Ethereal cards played, not cards exhausted by Ethereal.
pub(crate) fn after_played(c: &mut Combat, k: KRef, played: KRef) {
    if c.cards[k as usize].id == CardId::BansheesCry
        && owner(c, played) == owner(c, k) && c.cards[played as usize].is_ethereal()
    {
        let discount = var(c, k, VarKey::Energy, 2);
        c.cards[k as usize].add_combat_cost(-discount);
    }
}

/// Melancholy.AfterDeath: every non-prevented creature death, even an enemy or another
/// player's pet, lowers this combat-pile card's cost.
pub(crate) fn after_death(c: &mut Combat, k: KRef, prevented: bool) {
    if !prevented && c.cards[k as usize].id == CardId::Melancholy {
        let amount = var(c, k, VarKey::Energy, 1);
        c.cards[k as usize].add_combat_cost(-amount);
    }
}

/// RightHandHand.AfterCardPlayedLate: only return from discard, and only after the
/// owner's play actually spent the threshold energy.
pub(crate) fn late_effect(c: &mut Combat, k: KRef, played: KRef) {
    if c.cards[k as usize].id != CardId::RightHandHand {
        return;
    }
    let who = owner(c, k);
    if owner(c, played) == who
        && c.cards[played as usize].energy_spent >= var(c, k, VarKey::Energy, 2)
        && c.players[who].discard.contains(&k)
    {
        move_to_hand(c, k);
    }
}
