//! Every Regent card (r7 `cards/regent.py`) and the Regent tokens (`cards/status.py`: Sovereign
//! Blade, Minion Strike / Sacrifice / Dive Bomb, Debris), on the shared `kit` helpers and the
//! star/forge engine (`crate::regent`). Prism, Kaleidoscope and similar relics put these into
//! Ironclad runs. SOLAR_STRIKE stays in `cards_colorless.rs`.

use super::kit::*;
use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, PilePos, MAX_HAND_SIZE};
use crate::hooks::Ev;
use crate::tables::*;
use arrayvec::ArrayVec;
use smallvec::SmallVec;

#[cfg(test)]
mod astral_pulse_tests {
    use super::*;
    use crate::run::{RunManager,RunOptions};
    #[test]
    fn original_two_hits_apply_strength_and_block_per_hit_on_every_enemy() {
        for upgraded in [false,true] {
            let run=RunManager::new(7,RunOptions::default()).run_ctx().clone();
            let mut c=Combat::new(run,1);
            c.add_player(80,80,3,0,&[]);
            c.setup_encounter("CORPSE_SLUGS_NORMAL",&mut crate::rng::Rng::new(7));
            let enemies=c.hittable_enemies();
            assert!(!enemies.is_empty());
            for &enemy in &enemies {
                c.cr_mut(enemy).hp=100;
                c.cr_mut(enemy).max_hp=100;
                c.cr_mut(enemy).block=5;
            }
            c.apply_power_to(0,PowerId::Strength,2,None);
            let k=c.new_card(0,CardId::AstralPulse,upgraded).unwrap();
            assert!(play(&mut c,k,None));
            let hit=if upgraded {10}else{8}; // C# 6 (+2 upgraded), plus Strength.
            for enemy in enemies {
                assert_eq!(c.cr(enemy).hp,100-(2*hit-5));
                assert_eq!(c.cr(enemy).block,0);
            }
        }
    }
}

impl Combat {
    pub(crate) fn finish_constellation_draw(&mut self, card: KRef, target: CId) {
        let energy=var(self,card,VarKey::Energy,1);
        let block=self.cards[card as usize].block.unwrap_or(0);
        gain_energy(self,target,energy);
        block_target(self,card,target,block);
    }
}

fn resolver(id: CardId, k: KRef) -> Resolver {
    Resolver { src: ChoiceSrc::Card(id), k, target: None, step: 0, arg: 0 }
}

fn weak_vuln(c: &mut Combat, k: KRef, t: CId, weak: i32, vuln: i32) {
    let w = var(c, k, VarKey::Weak, weak);
    let v = var(c, k, VarKey::Vulnerable, vuln);
    c.apply_power_to(t, PowerId::Weak, w, None);
    c.apply_power_to(t, PowerId::Vulnerable, v, None);
}

fn stars(c: &mut Combat, k: KRef, default: i32) {
    let n = var(c, k, VarKey::Stars, default);
    let who = owner(c, k);
    c.gain_stars(who, n);
}

fn forge(c: &mut Combat, k: KRef, default: i32) {
    let n = var(c, k, VarKey::Forge, default);
    let who = owner(c, k);
    c.forge(who, n, Some(k));
}

fn colorless_cards(c: &mut Combat, who: CId, count: usize) -> SmallVec<[KRef; 10]> {
    c.generate_colorless(who, count)
}

fn generated_to_hand(c: &mut Combat, k: KRef) {
    c.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
}

/// `n` all-enemy hits, stopping once the owner is dead.
fn hit_all_n(c: &mut Combat, k: KRef, n: i32) {
    let who = owner(c, k);
    for _ in 0..n.max(0) {
        if c.cr(who).is_dead() {
            break;
        }
        hit_all(c, k);
    }
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, t: Option<CId>) -> bool {
    let id = c.cards[k as usize].id;
    let who = owner(c, k);
    let up = c.cards[k as usize].upgraded;
    match id {
        // ── BASIC ──
        CardId::StrikeRegent | CardId::KinglyKick | CardId::KinglyPunch | CardId::MakeItSo
        | CardId::Devastate | CardId::Bombardment | CardId::MinionDiveBomb => {
            hit(c, k, target(t));
        }
        CardId::DefendRegent | CardId::CloakOfStars | CardId::IAmInvincible | CardId::MinionSacrifice => {
            block(c, k);
        }
        CardId::FallingStar => {
            let tt = target(t);
            hit(c, k, tt);
            weak_vuln(c, k, tt, 1, 1);
        }
        CardId::Venerate => stars(c, k, 2),
        // ── COMMON ──
        // AstralPulse.OnPlay: DamageVar(6), WithHitCount(2), also when upgraded.
        CardId::AstralPulse => hit_all_n(c, k, 2),
        CardId::Begone => {
            if !c.players[who].hand.is_empty() {
                let hand: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::Constellation => {
            // C# Constellation.OnPlay: draw, energy and block all belong to the target.
            let tt = t.expect("CONSTELLATION without target");
            let n = var(c, k, VarKey::Cards, 1);
            c.draw_cards_for_branch(tt,n);
            if c.pending_choice.is_some() {
                c.pending_play.last_mut().expect("Constellation PendingPlay").post_draw_constellation=Some((k,tt));
            } else {
                c.finish_constellation_draw(k,tt);
            }
        }
        CardId::Plot => {
            // C# Plot.OnPlay applies DrawCardsNextTurn to every living teammate.
            let n = var(c, k, VarKey::Cards, 2);
            for player in 0..c.players.len() as CId {
                if c.cr(player).is_alive() {
                    c.apply_power_to(player, PowerId::DrawCardsNextTurn, n, Some(who));
                }
            }
        }
        CardId::Tutor => {
            // C# Tutor.OnPlay selects from cardPlay.Target.Player's draw pile.
            let Some(player) = t else {
                c.unported("Tutor requires an ally target");
                return true;
            };
            if !c.players[player].draw.is_empty() {
                let cards = draw_selector_order(c, player);
                c.request_card_choice(&cards, PileKind::Draw, resolver(id, k), false);
            }
        }
        CardId::CelestialMight => {
            let n = var(c, k, VarKey::Repeat, 3);
            hit_n(c, k, target(t), n);
        }
        CardId::CollisionCourse => {
            hit(c, k, target(t));
            if let Some(d) = c.new_card(who, CardId::Debris, false) {
                generated_to_hand(c, d);
            }
        }
        CardId::CosmicIndifference => {
            block(c, k);
            if !c.players[who].discard.is_empty() {
                let cards = c.players[who].discard.clone();
                c.request_card_choice(&cards, PileKind::Discard, resolver(id, k), false);
            }
        }
        CardId::CrescentSpear => {
            let count = [&c.players[who].hand[..], &c.players[who].draw[..], &c.players[who].discard[..], &c.players[who].play[..], &c.players[who].exhaust[..]]
                .iter()
                .flat_map(|p| p.iter())
                .filter(|&&q| {
                    let card = &c.cards[q as usize];
                    card.star_cost > 0 || card.star_x
                })
                .count() as i32;
            let base = var(c, k, VarKey::CalcBase, c.cards[k as usize].damage.unwrap_or(6));
            let extra = var(c, k, VarKey::ExtraDamage, 2);
            hit_base(c, k, target(t), base + extra * count);
        }
        CardId::CrushUnder => {
            let enemies = c.hittable_enemies();
            hit_all(c, k);
            let n = var(c, k, VarKey::StrengthLoss, 1);
            for e in enemies {
                c.apply_power_to(e, PowerId::CrushUnder, n, None);
            }
        }
        CardId::GatherLight => {
            block(c, k);
            stars(c, k, 1);
        }
        CardId::Glitterstream => {
            block(c, k);
            let next = var(c, k, VarKey::BlockNextTurn, 0);
            if next != 0 {
                let amount = c.calculate_block(next, who, crate::combat::Props::MOVE, Some(k));
                c.apply_power_to(who, PowerId::BlockNextTurn, amount, None);
            }
        }
        CardId::Glow => {
            let amount=var(c,k,VarKey::Stars,1);
            let count=var(c,k,VarKey::Cards,1);
            c.gain_stars(who,amount);
            c.draw_cards_at_end_of_play(who,count);
            if c.pending_choice.is_some()||c.signalled_setup_choice.is_some() {
                c.pending_play.last_mut().expect("Glow frame").post_draw_power=Some((who,PowerId::DrawCardsNextTurn,count));
            } else {c.apply_power_to(who,PowerId::DrawCardsNextTurn,count,Some(who));}
        }
        CardId::GuidingStar => {
            hit(c, k, target(t));
            let n = var(c, k, VarKey::Cards, 2);
            self_power(c, k, PowerId::DrawCardsNextTurn, n);
        }
        CardId::HiddenCache => {
            let n = var(c, k, VarKey::StarNextTurnPower, 3);
            self_power(c, k, PowerId::StarNextTurn, n);
            stars(c, k, 1);
        }
        CardId::KnowThyPlace => weak_vuln(c, k, target(t), 1, 1),
        CardId::Patter => {
            block(c, k);
            let n = var(c, k, VarKey::Vigor, 2);
            self_power(c, k, PowerId::Vigor, n);
        }
        CardId::PhotonCut => {
            hit(c, k, target(t));
            draw(c, k, var(c, k, VarKey::Cards, 1));
            if !c.players[who].hand.is_empty() {
                let hand: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::RefineBlade => {
            let n = var(c, k, VarKey::Energy, 1);
            self_power(c, k, PowerId::EnergyNextTurn, n);
            forge(c, k, 6);
        }
        CardId::SpoilsOfBattle => {
            forge(c, k, 6);
            c.draw_cards_at_end_of_play(who, var(c, k, VarKey::Cards, 2));
        }
        CardId::WroughtInWar => {
            hit(c, k, target(t));
            forge(c, k, 5);
        }
        // ── UNCOMMON ──
        CardId::Alignment => gain_energy(c, who, var(c, k, VarKey::Energy, 2)),
        CardId::BlackHole => {
            let n = var(c, k, VarKey::BlackHole, 3);
            self_power(c, k, PowerId::BlackHole, n);
        }
        CardId::Bulwark => {
            block(c, k);
            forge(c, k, 10);
        }
        CardId::Charge => {
            let cards = draw_selector_order(c, who);
            if !cards.is_empty() {
                let n = var(c, k, VarKey::Cards, 2) as u16;
                c.request_multi_card_choice(&cards, PileKind::Draw, resolver(id, k), n, None, false);
            }
        }
        CardId::ChildOfTheStars => {
            let n = var(c, k, VarKey::BlockForStars, 2);
            self_power(c, k, PowerId::ChildOfTheStars, n);
        }
        CardId::Conqueror => {
            c.apply_power_to(target(t), PowerId::Conqueror, 1, None);
            forge(c, k, 3);
        }
        CardId::Convergence => {
            self_power(c, k, PowerId::RetainHand, 1);
            let e = var(c, k, VarKey::Energy, 1);
            self_power(c, k, PowerId::EnergyNextTurn, e);
            let s = var(c, k, VarKey::Stars, 1);
            self_power(c, k, PowerId::StarNextTurn, s);
        }
        CardId::Furnace => {
            let n = var(c, k, VarKey::Forge, 4);
            self_power(c, k, PowerId::Furnace, n);
        }
        CardId::GammaBlast => {
            let tt = target(t);
            hit(c, k, tt);
            weak_vuln(c, k, tt, 2, 2);
        }
        CardId::Glimmer => {
            draw(c, k, var(c, k, VarKey::Cards, 3));
            if !c.players[who].hand.is_empty() {
                let hand: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::Hegemony => {
            hit(c, k, target(t));
            let n = var(c, k, VarKey::Energy, 2);
            self_power(c, k, PowerId::EnergyNextTurn, n);
        }
        CardId::KnockoutBlow => {
            let tt = target(t);
            hit(c, k, tt);
            if c.cr(tt).is_dead() {
                stars(c, k, 5);
            }
        }
        CardId::Largesse => {
            // C# Largesse.OnPlay / CardFactory.GetDistinctForCombat: target owns
            // the generated card; owner's run CombatCardGeneration supplies RNG.
            // ColorlessCardPool.GenerateAllCards order, minus Alchemize,
            // HandOfGreed and HiddenGem (CanBeGeneratedInCombat=false). Full unlocks
            // are the simulator's existing combat-generation contract.
            let Some(tt) = t else {
                c.unported("Largesse requires an ally target");
                return true;
            };
            let pool = crate::cardgen::character_pool(crate::character::CardPool::Colorless, c.players.len() > 1, None, None).expect("colorless pool");
            if let Some(&g) = c.generate_pool_cards(tt, pool, 1, true).first() {
                if up {
                    c.upgrade_card(g);
                }
                generated_to_hand(c, g);
            }
        }
        CardId::LunarBlast => {
            let n = plays_finished_of_type(c, who, CardType::Skill);
            hit_n(c, k, target(t), n);
        }
        CardId::ManifestAuthority => {
            block(c, k);
            if let Some(&g) = c.generate_unlocked_colorless_cards(who,1).first() {
                if up {
                    c.upgrade_card(g);
                }
                generated_to_hand(c, g);
            }
        }
        CardId::MonologueCard => self_power(c, k, PowerId::Monologue, 1),
        CardId::Orbit => self_power(c, k, PowerId::Orbit, 1),
        CardId::PaleBlueDot => {
            let n = var(c, k, VarKey::Cards, 1);
            self_power(c, k, PowerId::PaleBlueDot, n);
        }
        CardId::ParryCard => {
            let n = var(c, k, VarKey::Parry, 6);
            self_power(c, k, PowerId::Parry, n);
        }
        CardId::ParticleWall => {
            block(c, k);
            move_to_hand(c, k);
        }
        CardId::PillarOfCreation => {
            let n = var(c, k, VarKey::Block, 3);
            self_power(c, k, PowerId::PillarOfCreation, n);
        }
        CardId::Prophesize => draw(c, k, var(c, k, VarKey::Cards, 6)),
        CardId::Quasar => {
            let generated = colorless_cards(c, who,3);
            if up {
                for &g in &generated {
                    c.upgrade_card(g);
                }
            }
            if !generated.is_empty() {
                c.request_card_choice(&generated, PileKind::None, resolver(id, k), true);
            }
        }
        CardId::Radiate => {
            let n = c.hist.stars_gained_this_turn[who as usize];
            hit_all_n(c, k, n);
        }
        CardId::ReflectCard => {
            block(c, k);
            self_power(c, k, PowerId::Reflect, 1);
        }
        CardId::Resonance => {
            let n = var(c, k, VarKey::Strength, 1);
            self_power(c, k, PowerId::Strength, n);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Strength, -1, None);
            }
        }
        CardId::RoyalGamble => stars(c, k, 9),
        CardId::ShiningStrike => {
            hit(c, k, target(t));
            stars(c, k, 2);
            if !c.cards[k as usize].exhausts() {
                put_on_draw_top(c, k);
            }
        }
        CardId::SpectrumShift => self_power(c, k, PowerId::SpectrumShift, 1),
        CardId::Stardust => {
            let spent_all = c.players[who].stars;
            let mut hits = spent_all;
            for r in 0..c.run.relics.len() {
                hits = c.relic_modify_x_value(r, c.run.relics[r].owner, k, hits);
            }
            if hits > 0 {
                c.spend_stars(who, spent_all);
                for _ in 0..hits {
                    if c.cr(who).is_dead() {
                        break;
                    }
                    let alive = c.hittable_enemies();
                    if alive.is_empty() {
                        break;
                    }
                    let i = c.run.rngs.combat_targets.choice_index(alive.len());
                    hit(c, k, alive[i]);
                }
            }
        }
        CardId::SummonForth => {
            forge(c, k, 8);
            for pile in [PileKind::Draw, PileKind::Discard, PileKind::Exhaust] {
                let cards: Vec<KRef> = match pile {
                    PileKind::Draw => c.players[who].draw.clone(),
                    PileKind::Discard => c.players[who].discard.clone(),
                    _ => c.players[who].exhaust.clone(),
                };
                for q in cards {
                    if c.cards[q as usize].id == CardId::SovereignBlade && c.cards[q as usize].owner == who {
                        move_to_hand(c, q);
                    }
                }
            }
        }
        CardId::Supermassive => {
            let base = var(c, k, VarKey::CalcBase, c.cards[k as usize].damage.unwrap_or(5));
            let extra = var(c, k, VarKey::ExtraDamage, 3);
            let n = c.hist.generated_by_creator[who as usize];
            hit_base(c, k, target(t), base + extra * n);
        }
        CardId::Terraforming => {
            let n = var(c, k, VarKey::Vigor, 6);
            self_power(c, k, PowerId::Vigor, n);
        }
        // ── RARE ──
        CardId::Arsenal => {
            let n = var(c, k, VarKey::Arsenal, 1);
            self_power(c, k, PowerId::Arsenal, n);
        }
        CardId::BeatIntoShape => {
            let tt = target(t);
            let prior = c.hist.powered_hits_this_turn(who, tt);
            hit(c, k, tt);
            let base = var(c, k, VarKey::CalcBase, c.cards[k as usize].damage.unwrap_or(5));
            let extra = var(c, k, VarKey::CalcExtra, 5);
            c.forge(who, base + prior * extra, Some(k));
        }
        CardId::BigBang => {
            draw(c, k, var(c, k, VarKey::Cards, 1));
            stars(c, k, 1);
            gain_energy(c, who, var(c, k, VarKey::Energy, 1));
            forge(c, k, 5);
        }
        CardId::BundleOfJoy => {
            let n = var(c, k, VarKey::Cards, 3) as usize;
            for g in colorless_cards(c, who,n) {
                generated_to_hand(c, g);
            }
        }
        CardId::Comet => {
            let tt = target(t);
            hit(c, k, tt);
            weak_vuln(c, k, tt, 3, 3);
        }
        CardId::CrashLanding => {
            hit_all(c, k);
            let n = MAX_HAND_SIZE.saturating_sub(c.players[who].hand.len());
            for _ in 0..n {
                if let Some(d) = c.new_card(who, CardId::Debris, false) {
                    generated_to_hand(c, d);
                }
            }
        }
        CardId::DecisionsDecisions => {
            draw(c, k, var(c, k, VarKey::Cards, 3));
            let skills: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&q| c.cards[q as usize].card_type == CardType::Skill && !c.cards[q as usize].is_unplayable())
                .collect();
            if !skills.is_empty() {
                c.request_card_choice(&skills, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::DyingStar => {
            hit_all(c, k);
            let n = var(c, k, VarKey::StrengthLoss, 9);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::DyingStar, n, None);
            }
        }
        CardId::ForegoneConclusion => {
            let n = var(c, k, VarKey::Cards, 2);
            self_power(c, k, PowerId::ForegoneConclusion, n);
        }
        CardId::Genesis => {
            let n = var(c, k, VarKey::StarsPerTurn, 2);
            self_power(c, k, PowerId::Genesis, n);
        }
        CardId::Guards => {
            if !c.players[who].hand.is_empty() {
                let hand: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                let n = hand.len() as u16;
                c.request_multi_card_choice(&hand, PileKind::Hand, resolver(id, k), 0, Some(n), true);
            }
        }
        CardId::HammerTime => self_power(c, k, PowerId::HammerTime, 1),
        CardId::HeavenlyDrill => {
            let mut x = c.cards[k as usize].energy_spent;
            if x >= var(c, k, VarKey::Energy, 4) {
                x *= 2;
            }
            hit_n(c, k, target(t), x);
        }
        // C# HeirloomHammer.OnPlay: hit, then pick one hand card whose VisualCardPool is
        // colorless and add Repeat clones of it to the hand.
        CardId::HeirloomHammer => {
            hit(c, k, target(t));
            let candidates: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&q| {
                    let card = &c.cards[q as usize];
                    card_info(card.id, card.upgraded).is_some_and(|i| i.visual_colorless)
                })
                .collect();
            if !candidates.is_empty() && !c.is_over_or_ending() {
                c.request_card_choice(&candidates, PileKind::Hand, resolver(id, k), false);
            }
        }
        CardId::MonarchsGazeCard => self_power(c, k, PowerId::MonarchsGaze, 1),
        CardId::NeutronAegis => {
            let n = var(c, k, VarKey::Plating, 8);
            self_power(c, k, PowerId::Plating, n);
        }
        CardId::RoyaltiesCard => self_power(c, k, PowerId::Royalties, 1),
        CardId::SeekingEdge => {
            forge(c, k, 7);
            self_power(c, k, PowerId::SeekingEdge, 1);
        }
        CardId::SevenStars => {
            let n = var(c, k, VarKey::Repeat, 7);
            hit_all_n(c, k, n);
        }
        CardId::SwordSage => self_power(c, k, PowerId::SwordSage, 1),
        CardId::TheSmith => forge(c, k, 30),
        CardId::TyrannyCard => self_power(c, k, PowerId::Tyranny, 1),
        CardId::VoidForm => {
            let n = var(c, k, VarKey::VoidFormPower, 2);
            self_power(c, k, PowerId::VoidForm, n);
            c.end_turn_after_play = true;
        }
        // ── ANCIENT ──
        CardId::MeteorShower => {
            hit_all(c, k);
            for e in c.hittable_enemies() {
                weak_vuln(c, k, e, 2, 2);
            }
        }
        CardId::TheSealedThrone => self_power(c, k, PowerId::TheSealedThrone, 1),
        // ── TOKENS (cards/status.py) ──
        CardId::MinionStrike => {
            hit(c, k, target(t));
            draw(c, k, var(c, k, VarKey::Cards, 1));
        }
        // C# SovereignBlade.OnPlay: one attack with HitCount = Repeat (1; Sword Sage adds
        // replays instead, see Combat::sword_sage_replays).
        CardId::SovereignBlade => {
            if c.has_power(who, PowerId::SeekingEdge) {
                hit_all_n(c, k, 1);
            } else if let Some(tt) = t {
                hit(c, k, tt);
            }
        }
        _ => return false,
    }
    true
}

/// Choice continuations.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    let ChoiceSrc::Card(id) = r.src else { return false };
    // ForegoneConclusionPower's search_draw_pile_to_hand (step 1: no card behind it).
    if id == CardId::ForegoneConclusion && r.step == 1 {
        for &q in selected {
            move_to_hand(c, q);
        }
        return true;
    }
    // TyrannyPower's request_exhaust (step 1: no card behind it).
    if id == CardId::TyrannyCard && r.step == 1 {
        for &q in selected {
            if c.players[owner(c, q)].hand.contains(&q) {
                exhaust(c, q);
            }
        }
        return true;
    }
    let up = c.cards[r.k as usize].upgraded;
    match id {
        CardId::Begone => {
            if let Some(&q) = selected.first() {
                transform_in_pile(c, q, CardId::MinionStrike, up);
            }
        }
        CardId::Charge => {
            for &q in selected {
                // Charge.OnPlay transforms to a BASE MinionDiveBomb; generated-card
                // callbacks run before CardCmd.Upgrade on the returned replacement.
                if let Some(new) = transform_in_pile(c, q, CardId::MinionDiveBomb, false) {
                    if up { c.upgrade_card(new); }
                }
            }
        }
        CardId::Guards => {
            for &q in selected {
                transform_in_pile(c, q, CardId::MinionSacrifice, up);
            }
        }
        CardId::HeirloomHammer => {
            if let Some(&q) = selected.first() {
                for _ in 0..var(c, r.k, VarKey::Repeat, 1).max(0) {
                    c.clone_card_to_hand(q);
                }
            }
        }
        CardId::Tutor => {
            if let Some(&q) = selected.first() {
                move_to_hand(c, q);
            }
        }
        CardId::CosmicIndifference | CardId::PhotonCut | CardId::Glimmer => {
            if let Some(&q) = selected.first() {
                put_on_draw_top(c, q);
            }
        }
        // Quasar: `move_card_to_hand` of a generated card (was not in combat: entered-combat
        // hooks run; r7 records no generation).
        CardId::Quasar => {
            if let Some(&q) = selected.first()
                && !c.is_over_or_ending()
            {
                let who = owner(c, r.k);
                let p = &mut c.players[who];
                if p.hand.len() < MAX_HAND_SIZE {
                    p.hand.push(q);
                } else {
                    p.discard.push(q);
                }
                c.fire(Hook::AfterCardEnteredCombat, Ev::Card(q), None);
                c.fire_card_after_card_entered_combat(q);
                c.fire_after_card_changed_piles(q, PileKind::None, None);
            }
        }
        CardId::DecisionsDecisions => {
            if let Some(&q) = selected.first() {
                let n = var(c, r.k, VarKey::Repeat, 3);
                // DecisionsDecisions.OnPlay awaits every repeated AutoPlay.
                c.autoplay_snapshot(vec![q; n.max(0) as usize]);
            }
        }
        _ => return false,
    }
    true
}

/// KINGLY_KICK / KINGLY_PUNCH `after_card_drawn` (C# AfterCardDrawn: only when it is drawn).
pub(crate) fn on_after_card_drawn(c: &mut Combat, listener: KRef, drawn: KRef) {
    if listener != drawn {
        return;
    }
    match c.cards[listener as usize].id {
        // EnergyCost.AddThisCombat(-1): r7 set_combat_cost(max(0, cost - 1)).
        CardId::KinglyKick => {
            let card = &mut c.cards[listener as usize];
            card.add_combat_cost(-1);
        }
        CardId::KinglyPunch => {
            let n = var(c, listener, VarKey::Increase, 3);
            increase_damage(c, listener, n);
        }
        _ => {}
    }
}

/// BOMBARDMENT `before_hand_draw`: auto-plays itself from the exhaust pile.
pub(crate) fn on_before_hand_draw(c: &mut Combat, listener: KRef, drawing_owner: CId) {
    if c.cards[listener as usize].id == CardId::Bombardment
        && c.cards[listener as usize].owner == drawing_owner
        && c.players[drawing_owner].exhaust.contains(&listener)
    {
        c.auto_play_card(listener, None, false);
    }
}

/// C# IAmInvincible.AfterAutoPostPlayPhaseEntered: only its owner's top draw card.
pub(crate) fn on_after_auto_post_play(c: &mut Combat, listener: KRef, player: CId) {
    if c.cards[listener as usize].id != CardId::IAmInvincible {
        return;
    }
    let who = owner(c, listener);
    if player == who && c.players[who].draw.first() == Some(&listener) {
        // IAmInvincible.cs:35-36 uses CardPileCmd.AutoPlayFromDrawPile (Top, 1).
        c.auto_play_from_draw(who, 1, false);
    }
}

/// MAKE_IT_SO late effect: returns to hand on every `cards`-th Skill played this turn.
pub(crate) fn on_late_effect(c: &mut Combat, watched: KRef, played: KRef) {
    if c.cards[watched as usize].id != CardId::MakeItSo {
        return;
    }
    let who = owner(c, watched);
    if c.cards[played as usize].owner != who || c.cards[played as usize].card_type != CardType::Skill {
        return;
    }
    if c.players[who].hand.contains(&watched) {
        return;
    }
    let interval = var(c, watched, VarKey::Cards, 3);
    if interval > 0 && plays_finished_of_type(c, who, CardType::Skill) % interval == 0 {
        move_to_hand(c, watched);
    }
}

/// SOVEREIGN_BLADE target type: all enemies while Seeking Edge is active.
pub(crate) fn target_type(c: &Combat, k: KRef) -> Option<TargetType> {
    let card = &c.cards[k as usize];
    (card.id == CardId::SovereignBlade && c.has_power(card.owner, PowerId::SeekingEdge)).then_some(TargetType::AllEnemies)
}
