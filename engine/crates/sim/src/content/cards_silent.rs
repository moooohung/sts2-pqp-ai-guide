//! r7 `cards/silent.py` (every registered Silent card) plus the SHIV token
//! (`cards/status.py:1191-1204`). Prism, Kaleidoscope and similar relics put these cards into an
//! Ironclad run. Silent's own helpers deal and block without the generic card helpers' owner-dead /
//! combat-over guards, so they are written out here exactly like r7.

use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, Props};
use crate::hooks::Ev;
use crate::tables::*;
use arrayvec::ArrayVec;

/// r7 `LEADING_STRIKE_SHIVS`.
const LEADING_STRIKE_SHIVS: i32 = 1;
/// r7 `_make_shiv(upgraded=True).base_damage` (KNIFE_TRAP upgrades exhausted shivs in place).
const UPGRADED_SHIV_DAMAGE: i32 = 6;

#[inline]
fn owner(c: &Combat, k: KRef) -> CId {
    c.cards[k as usize].owner
}

#[inline]
fn var(c: &Combat, k: KRef, key: VarKey, default: i32) -> i32 {
    c.cards[k as usize].var(key).unwrap_or(default)
}

#[inline]
fn attack_target(target: Option<CId>) -> CId {
    target.expect("ANY_ENEMY card without target")
}

/// `calculate_damage(base, owner, target, MOVE)` + `apply_damage(...)`.
fn hit_base(c: &mut Combat, k: KRef, t: CId, base: i32) {
    let who = owner(c, k);
    let dmg = c.calculate_damage(base, Some(who), t, Props::MOVE);
    c.apply_damage(t, dmg, Props::MOVE, Some(who));
}

fn hit(c: &mut Combat, k: KRef, t: CId) {
    let base = c.cards[k as usize].damage.unwrap_or(0);
    hit_base(c, k, t, base);
}

fn hit_all(c: &mut Combat, k: KRef) {
    c.deal_card_damage_all(k);
}

/// `_gain_resolved_block(owner, calculate_block(base, owner, MOVE, card_source=card))`.
fn block_base(c: &mut Combat, k: KRef, base: i32) -> i32 {
    let who = owner(c, k);
    let amount = c.calculate_block(base, who, Props::MOVE, Some(k));
    let before = c.cr(who).block;
    c.gain_recorded_block(who,amount);
    let gained = c.cr(who).block - before;
    if gained > 0 {
        c.record_block_gained_event(who, Props::MOVE, c.active_card);
        c.fire(Hook::AfterBlockGained, Ev::BlockGained { creature: who, amount: gained }, None);
    }
    gained
}

fn block(c: &mut Combat, k: KRef) -> i32 {
    let base = c.cards[k as usize].block.unwrap_or(0);
    block_base(c, k, base)
}

use super::kit::gain_energy;

fn self_power(c: &mut Combat, k: KRef, id: PowerId, amount: i32) {
    let who = owner(c, k);
    c.apply_power_to(who, id, amount, None);
}

fn shivs(c: &mut Combat, who: CId, count: i32, upgraded: bool) {
    for _ in 0..count.max(0) {
        if let Some(k) = c.new_card(who, CardId::Shiv, upgraded) {
            // silent.py:_make_shiv has no effect_vars, unlike status.py:make_shiv.
            // C# Shiv.CanonicalVars exposes Damage only; these factory helper
            // parameters are not card mutations and must not become wire rows.
            c.cards[k as usize].vars.retain(|(key,_)|
                !matches!(key, VarKey::CalcBase | VarKey::CalcExtra));
            c.add_generated_card(k, PileKind::Hand, crate::combat::PilePos::Bottom, true);
        }
    }
}

fn resolver(id: CardId, k: KRef, step: u8) -> Resolver {
    Resolver { src: ChoiceSrc::Card(id), k, target: None, step, arg: 0 }
}

/// "Choose a hand card to discard" -> `discard_cards([selected])`.
fn discard_one_choice(c: &mut Combat, k: KRef) {
    let who = owner(c, k);
    if c.players[who].hand.is_empty() {
        return;
    }
    let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
    let id = c.cards[k as usize].id;
    c.request_card_choice(&hand, PileKind::Hand, resolver(id, k, 0), false);
}

impl Combat {
    pub(crate) fn request_draw_followup_discard(&mut self,k:KRef) {
        if self.cards[k as usize].id!=CardId::Prepared {
            discard_one_choice(self,k);
            return;
        }
        let who=owner(self,k);
        let count=var(self,k,VarKey::Cards,1).min(self.players[who].hand.len() as i32);
        if count<=0 {return;}
        let hand:ArrayVec<KRef,{crate::combat::MAX_HAND_SIZE}>=self.players[who].hand.iter().copied().collect();
        if count==1 {
            self.request_card_choice(&hand,PileKind::Hand,resolver(CardId::Prepared,k,0),false);
        } else {
            self.request_multi_card_choice(&hand,PileKind::Hand,resolver(CardId::Prepared,k,0),
                count as u16,Some(count as u16),false);
        }
    }
}

/// `count_card_plays_finished_this_turn(owner, card_type=...)`.
fn plays_finished(c: &Combat, who: CId, card_type: CardType) -> i32 {
    c.hist
        .played_this_turn
        .iter()
        .filter(|&&(actor, q)| actor == who && c.cards[q as usize].card_type == card_type)
        .count() as i32
}

/// HIDDEN_DAGGERS `_finish`.
fn hidden_daggers_finish(c: &mut Combat, k: KRef) {
    let n = var(c, k, VarKey::Shivs, 2);
    let up = c.cards[k as usize].upgraded;
    shivs(c, owner(c, k), n, up);
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, target: Option<CId>) -> bool {
    let id = c.cards[k as usize].id;
    let who = owner(c, k);
    match id {
        // ── BASIC ──
        CardId::StrikeSilent | CardId::Slice | CardId::Backstab | CardId::Pinpoint => hit(c, k, attack_target(target)),
        CardId::DefendSilent | CardId::Deflect | CardId::Untouchable => {
            block(c, k);
        }
        CardId::Neutralize | CardId::SuckerPunch => {
            let t = attack_target(target);
            hit(c, k, t);
            let weak = var(c, k, VarKey::Weak, 1);
            c.apply_power_to(t, PowerId::Weak, weak, None);
        }
        CardId::Survivor => {
            block(c, k);
            discard_one_choice(c, k);
        }
        // ── COMMON ──
        CardId::Acrobatics => {
            let n = var(c, k, VarKey::Cards, 3);
            c.draw_cards(who, n, false);
            discard_one_choice(c, k);
        }
        CardId::Anticipate => {
            let n = var(c, k, VarKey::Dexterity, 3);
            self_power(c, k, PowerId::Anticipate, n);
        }
        CardId::Backflip => {
            block(c, k);
            let n = var(c, k, VarKey::Cards, 2);
            c.draw_cards(who, n, false);
        }
        CardId::BladeDance => {
            let n = var(c, k, VarKey::Cards, 3);
            shivs(c, who,n, false);
        }
        CardId::CloakAndDagger => {
            block(c, k);
            let n = var(c, k, VarKey::Cards, 1);
            shivs(c, who,n, false);
        }
        CardId::DaggerSpray => {
            for _ in 0..2 {
                if c.cr(who).is_dead() {
                    break;
                }
                hit_all(c, k);
            }
        }
        CardId::DaggerThrow => {
            hit(c, k, attack_target(target));
            // C# awaits Draw (including shuffle/draw listeners) before the
            // hand-discard selector. Never overwrite a suspended draw choice.
            c.draw_cards_at_end_of_play(who,1);
            if c.pending_choice.is_some() || c.signalled_setup_choice.is_some() {
                c.pending_play.last_mut().expect("Dagger Throw play frame")
                    .post_draw_discard_card=Some(k);
            } else {
                discard_one_choice(c,k);
            }
        }
        CardId::DeadlyPoison | CardId::Snakebite => {
            let n = var(c, k, VarKey::PoisonPower, if id == CardId::Snakebite { 7 } else { 5 });
            c.apply_power_to(attack_target(target), PowerId::Poison, n, None);
        }
        CardId::DodgeAndRoll => {
            let gained = block(c, k);
            c.apply_power_to(who, PowerId::BlockNextTurn, gained, None);
        }
        CardId::FlickFlack | CardId::GrandFinale => hit_all(c, k),
        CardId::LeadingStrike => {
            hit(c, k, attack_target(target));
            let n = var(c, k, VarKey::Shivs, LEADING_STRIKE_SHIVS);
            shivs(c, who,n, false);
        }
        CardId::PiercingWail => {
            let n = var(c, k, VarKey::StrengthLoss, 6);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::PiercingWail, n, None);
            }
        }
        CardId::PoisonedStab => {
            let t = attack_target(target);
            hit(c, k, t);
            let n = var(c, k, VarKey::PoisonPower, 3);
            c.apply_power_to(t, PowerId::Poison, n, None);
        }
        CardId::Prepared => {
            let draw = var(c, k, VarKey::Cards, 1);
            // Prepared.OnPlay awaits the complete draw before hand selection.
            c.draw_cards_at_end_of_play(who, draw);
            if c.pending_choice.is_some() || c.signalled_setup_choice.is_some() {
                c.pending_play.last_mut().expect("Prepared play frame")
                    .post_draw_discard_card=Some(k);
            } else {
                c.request_draw_followup_discard(k);
            }
        }
        CardId::Ricochet => {
            for _ in 0..var(c, k, VarKey::Repeat, 4) {
                if c.cr(who).is_dead() {
                    break;
                }
                let alive = c.hittable_enemies();
                if alive.is_empty() {
                    break;
                }
                let t = alive[c.run.rngs.combat_targets.choice_index(alive.len())];
                hit(c, k, t);
            }
        }
        // ── UNCOMMON ──
        CardId::AccuracyCard => {
            let n = var(c, k, VarKey::AccuracyPower, 4);
            self_power(c, k, PowerId::Accuracy, n);
        }
        CardId::BlurCard => {
            block(c, k);
            let n = var(c, k, VarKey::Blur, 1);
            self_power(c, k, PowerId::Blur, n);
        }
        CardId::BouncingFlask => {
            let repeat = var(c, k, VarKey::Repeat, 3);
            let n = var(c, k, VarKey::PoisonPower, 3);
            for _ in 0..repeat {
                let alive = c.hittable_enemies();
                if alive.is_empty() {
                    break;
                }
                let t = alive[c.run.rngs.combat_targets.choice_index(alive.len())];
                c.apply_power_to(t, PowerId::Poison, n, None);
            }
        }
        CardId::BubbleBubble => {
            let t = attack_target(target);
            if c.power_amount(t, PowerId::Poison) > 0 {
                let n = var(c, k, VarKey::PoisonPower, 9);
                c.apply_power_to(t, PowerId::Poison, n, None);
            }
        }
        CardId::CalculatedGamble => {
            let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
            c.discard_cards(who, &hand, hand.len() as i32);
        }
        CardId::Dash => {
            let t = attack_target(target);
            block(c, k);
            hit(c, k, t);
        }
        CardId::EscapePlan => {
            let pre = c.players[who].hand.len();
            c.draw_cards(who, 1, false);
            if c.players[who].hand.len() <= pre {
                return true;
            }
            let drawn = c.players[who].hand[pre];
            if c.cards[drawn as usize].card_type == CardType::Skill {
                block(c, k);
            }
        }
        CardId::Expertise => {
            // Expertise.cs: await Draw(2/3), then retain exactly its results.
            c.draw_cards_then_retain(who,if c.cards[k as usize].upgraded {3}else{2});
        }
        CardId::Expose => {
            let t = attack_target(target);
            let n = var(c, k, VarKey::Power, 2);
            c.cr_mut(t).block = 0;
            if let Some(p) = c.find_power(t, PowerId::Artifact) {
                c.remove_power(t, p);
            }
            c.apply_power_to(t, PowerId::Vulnerable, n, None);
        }
        CardId::Finisher => {
            let t = attack_target(target);
            for _ in 0..plays_finished(c, who, CardType::Attack) {
                if c.cr(who).is_dead() || c.cr(t).is_dead() {
                    break;
                }
                hit(c, k, t);
            }
        }
        CardId::Flanking => {
            let t = attack_target(target);
            let n = var(c, k, VarKey::FlankingPower, 2);
            c.apply_power_to(t, PowerId::Flanking, n, Some(who));
        }
        CardId::Flechettes => {
            let t = attack_target(target);
            let skills = c.players[who].hand.iter().filter(|&&q| c.cards[q as usize].card_type == CardType::Skill).count();
            for _ in 0..skills {
                if c.cr(who).is_dead() || c.cr(t).is_dead() {
                    break;
                }
                hit(c, k, t);
            }
        }
        CardId::FollowThrough => {
            hit_all(c, k);
            // `last_card_play_started_this_turn(owner, exclude_card=card)`.
            let previous = c
                .hist
                .play_start_cards
                .iter()
                .rev()
                .copied()
                .find(|&q| q != k && c.cards[q as usize].owner == who);
            if previous.is_none_or(|q| c.cards[q as usize].card_type != CardType::Skill) {
                return true;
            }
            let n = var(c, k, VarKey::Weak, 1);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Weak, n, None);
            }
        }
        CardId::Footwork => {
            let n = var(c, k, VarKey::Dexterity, 2);
            self_power(c, k, PowerId::Dexterity, n);
        }
        CardId::HandTrick => {
            block(c, k);
            let candidates: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&q| c.cards[q as usize].card_type == CardType::Skill && !c.cards[q as usize].is_sly())
                .collect();
            if !candidates.is_empty() {
                c.request_card_choice(&candidates, PileKind::Hand, resolver(id, k, 0), false);
            }
        }
        CardId::Haze => {
            let n = var(c, k, VarKey::PoisonPower, 4);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Poison, n, None);
            }
        }
        CardId::HiddenDaggers => {
            let n = var(c, k, VarKey::Cards, 2).min(c.players[who].hand.len() as i32);
            if n <= 0 {
                hidden_daggers_finish(c, k);
                return true;
            }
            let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
            if n == 1 {
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k, 0), false);
            } else {
                c.request_multi_card_choice(&hand, PileKind::Hand, resolver(id, k, 0), n as u16, Some(n as u16), false);
            }
        }
        CardId::InfiniteBladesCard => self_power(c, k, PowerId::InfiniteBlades, 1),
        CardId::LegSweep => {
            let t = attack_target(target);
            block(c, k);
            let n = var(c, k, VarKey::Weak, 2);
            c.apply_power_to(t, PowerId::Weak, n, None);
        }
        CardId::MementoMori => {
            let t = attack_target(target);
            let base = var(c, k, VarKey::CalcBase, 8) + var(c, k, VarKey::ExtraDamage, 4) * c.hist.discarded_this_turn;
            hit_base(c, k, t, base);
        }
        CardId::Mirage => {
            let total: i32 = c.alive_enemies().iter().map(|&e| c.power_amount(e, PowerId::Poison)).sum();
            let amount = total * var(c, k, VarKey::CalcExtra, 1);
            block_base(c, k, amount);
        }
        CardId::NoxiousFumesCard => {
            let n = var(c, k, VarKey::PoisonPerTurn, 2);
            self_power(c, k, PowerId::NoxiousFumes, n);
        }
        CardId::OutbreakCard => {
            // Outbreak.OnPlay applies Poison to the complete hittable-enemy
            // batch, then triggers each Poison. Doom resolves at side-end via
            // DoomPower, not an r7-only Hextech instant-death guard.
            let n = var(c, k, VarKey::Poison, 9);
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Poison, n, Some(who));
            }
            for e in c.hittable_enemies() {
                if c.has_power(e, PowerId::Poison) {
                    c.poison_trigger(e);
                }
            }
        }
        CardId::PhantomBladesCard => {
            let n = var(c, k, VarKey::PhantomBladesPower, 9);
            self_power(c, k, PowerId::PhantomBlades, n);
            for q in c.pile_cards(who) {
                if c.cards[q as usize].has_tag(CardTag::Shiv) {
                    c.cards[q as usize].keywords |= KW_RETAIN;
                }
            }
        }
        CardId::Pounce => {
            hit(c, k, attack_target(target));
            self_power(c, k, PowerId::FreeSkill, 1);
        }
        CardId::PreciseCut => {
            let t = attack_target(target);
            let base = (var(c, k, VarKey::CalcBase, 13) - var(c, k, VarKey::ExtraDamage, 2) * c.players[who].hand.len() as i32).max(0);
            hit_base(c, k, t, base);
        }
        CardId::Predator => {
            hit(c, k, attack_target(target));
            self_power(c, k, PowerId::DrawCardsNextTurn, 2);
        }
        CardId::Reflex => {
            let n = var(c, k, VarKey::Cards, 2);
            c.draw_cards(who, n, false);
        }
        CardId::Skewer => {
            let t = attack_target(target);
            for _ in 0..c.cards[k as usize].energy_spent.max(0) {
                if c.cr(who).is_dead() || c.cr(t).is_dead() {
                    break;
                }
                hit(c, k, t);
            }
        }
        CardId::SpeedsterCard => {
            let n = var(c, k, VarKey::SpeedsterPower, 2);
            self_power(c, k, PowerId::Speedster, n);
        }
        CardId::Strangle => {
            let t = attack_target(target);
            hit(c, k, t);
            let n = var(c, k, VarKey::StranglePower, 2);
            // Strangle.cs OnPlay applies StranglePower (instanced per applier),
            // not the unrelated ConstrictPower's end-turn damage.
            c.apply_power_to(t, PowerId::Strangle, n, Some(who));
        }
        CardId::Tactician => gain_energy(c, who, var(c, k, VarKey::Energy, 1)),
        CardId::UpMySleeve => {
            let n = var(c, k, VarKey::Cards, 3);
            shivs(c, who,n, false);
            let card = &mut c.cards[k as usize];
            card.add_combat_cost(-1);
        }
        CardId::WellLaidPlans => {
            // WellLaidPlans.OnPlay always applies 1; OnUpgrade only reduces cost.
            self_power(c, k, PowerId::WellLaidPlans, 1);
        }
        // ── RARE ──
        CardId::Abrasive => {
            let dex = var(c, k, VarKey::Dexterity, 1);
            let thorns = var(c, k, VarKey::ThornsPower, 4);
            self_power(c, k, PowerId::Dexterity, dex);
            self_power(c, k, PowerId::Thorns, thorns);
        }
        CardId::Accelerant => {
            let n = var(c, k, VarKey::Accelerant, 1);
            self_power(c, k, PowerId::Accelerant, n);
        }
        CardId::Adrenaline => {
            gain_energy(c, who, var(c, k, VarKey::Energy, 1));
            let n = var(c, k, VarKey::Cards, 2);
            c.draw_cards(who, n, false);
        }
        CardId::AfterimageCard => {
            let n = var(c, k, VarKey::AfterimagePower, 1);
            self_power(c, k, PowerId::Afterimage, n);
        }
        CardId::Assassinate => {
            let t = attack_target(target);
            hit(c, k, t);
            let n = var(c, k, VarKey::Vulnerable, 1);
            c.apply_power_to(t, PowerId::Vulnerable, n, None);
        }
        CardId::BladeOfInk => {
            for _ in 0..var(c, k, VarKey::Cards, 2).max(0) {
                if let Some(s) = c.new_card(who, CardId::Shiv, false) {
                    crate::card::enchant::apply_static_enchantment(&mut c.cards[s as usize], "Inky", 1);
                    c.add_generated_card(s, PileKind::Hand, crate::combat::PilePos::Bottom, true);
                }
            }
        }
        CardId::BladeSymphony => {
            // C# BladeSymphony.OnPlay: GetTeammatesOf includes every living player.
            let n = var(c, k, VarKey::Cards, 2);
            for player in 0..c.players.len() as CId {
                if c.cr(player).is_alive() {
                    // r7 BladeSymphony explicitly uses status.py:make_shiv.
                    for _ in 0..n.max(0) { c.add_shiv_to_hand(player, false); }
                }
            }
        }
        CardId::Concoct => {
            let n = var(c, k, VarKey::ConcoctPower, 3);
            c.apply_power_to(attack_target(target), PowerId::Concoct, n, Some(who));
        }
        CardId::Fade => {
            // C# Fade.OnPlay applies its own inherited temporary-Dexterity power.
            let Some(t) = target else {
                c.unported("Fade requires an ally target");
                return true;
            };
            let n = var(c, k, VarKey::Dexterity, 6);
            c.apply_power_to(t, PowerId::Fade, n, Some(who));
        }
        CardId::Sidestep => {
            let n = var(c, k, VarKey::Energy, 1);
            c.apply_power_to(who, PowerId::EnergyNextTurn, n, Some(who));
        }
        // C# Outmaneuver.OnPlay.
        CardId::Outmaneuver => {
            c.apply_power_to(who, PowerId::EnergyNextTurn, var(c, k, VarKey::Energy, 2), Some(who));
        }
        CardId::BulletTime => {
            self_power(c, k, PowerId::NoDraw, 1);
            let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
            for q in hand {
                if !c.cards[q as usize].x_cost {
                    c.set_temporary_free_this_turn(q);
                }
            }
        }
        CardId::Burst => {
            let n = var(c, k, VarKey::Skills, 1);
            self_power(c, k, PowerId::Burst, n);
        }
        CardId::CorrosiveWave => {
            let n = var(c, k, VarKey::CorrosiveWave, 3);
            self_power(c, k, PowerId::CorrosiveWave, n);
        }
        CardId::EchoingSlash => {
            let mut attacks = 1;
            while attacks > 0 {
                if c.cr(who).is_dead() {
                    break;
                }
                attacks -= 1;
                let mut killed = 0;
                for e in c.hittable_enemies() {
                    hit(c, k, e);
                    if c.cr(e).is_dead() {
                        killed += 1;
                    }
                }
                attacks += killed;
            }
        }
        CardId::EnvenomCard => {
            let n = var(c, k, VarKey::EnvenomPower, 1);
            self_power(c, k, PowerId::Envenom, n);
        }
        CardId::FanOfKnivesCard => {
            self_power(c, k, PowerId::FanOfKnives, 1);
            let n = var(c, k, VarKey::Shivs, 2);
            shivs(c, who,n, false);
        }
        CardId::KnifeTrap => {
            let up = c.cards[k as usize].upgraded;
            let exhausted: Vec<KRef> =
                c.players[who].exhaust.iter().copied().filter(|&q| c.cards[q as usize].has_tag(CardTag::Shiv)).collect();
            for s in exhausted {
                if up && !c.cards[s as usize].upgraded {
                    c.cards[s as usize].upgraded = true;
                    c.cards[s as usize].damage = Some(UPGRADED_SHIV_DAMAGE);
                }
                if let Some(t) = target
                    && c.cr(t).is_alive()
                {
                    let base = c.cards[s as usize].damage.unwrap_or(0);
                    hit_base(c, k, t, base);
                }
            }
        }
        CardId::Malaise => {
            let t = attack_target(target);
            let x = c.cards[k as usize].energy_spent + i32::from(c.cards[k as usize].upgraded);
            c.apply_power_to(t, PowerId::Strength, -x, None);
            c.apply_power_to(t, PowerId::Weak, x, None);
        }
        CardId::MasterPlanner => self_power(c, k, PowerId::MasterPlanner, 1),
        CardId::Murder => {
            let t = attack_target(target);
            let base = var(c, k, VarKey::CalcBase, 1) * var(c, k, VarKey::ExtraDamage, 1) * c.hist.cards_drawn_combat;
            hit_base(c, k, t, base);
        }
        CardId::Nightmare => {
            if !c.players[who].hand.is_empty() {
                let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
                c.request_card_choice(&hand, PileKind::Hand, resolver(id, k, 0), false);
            }
        }
        CardId::SerpentFormCard => {
            let n = var(c, k, VarKey::SerpentFormPower, 4);
            self_power(c, k, PowerId::SerpentForm, n);
        }
        CardId::ShadowStep => {
            let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
            c.discard_cards(who, &hand, 0);
            self_power(c, k, PowerId::ShadowStep, 1);
        }
        CardId::Shadowmeld => {
            let n = var(c, k, VarKey::Power, 1);
            self_power(c, k, PowerId::Shadowmeld, n);
        }
        CardId::SneakyCard => {
            let n = var(c, k, VarKey::SneakyPower, 1);
            self_power(c, k, PowerId::Sneaky, n);
        }
        CardId::StormOfSteel => {
            let hand: ArrayVec<KRef, { crate::combat::MAX_HAND_SIZE }> = c.players[who].hand.iter().copied().collect();
            let count = hand.len() as i32;
            c.discard_cards(who, &hand, 0);
            let up = c.cards[k as usize].upgraded;
            shivs(c, who,count, up);
        }
        CardId::TheHunt => {
            let t = attack_target(target);
            let fatal = c.should_owner_death_trigger_fatal(t);
            hit(c, k, t);
            if fatal && c.cr(t).is_dead() {
                // C# TheHunt.cs:47 appends an owner-scoped CardReward in the
                // room's ordered extras, with the current combat room's odds.
                c.end_rewards.push(crate::combat::EndReward::Card {
                    owner: who, room: c.run.room_type,
                });
                self_power(c, k, PowerId::TheHunt, 1);
            }
        }
        CardId::ToolsOfTheTrade => self_power(c, k, PowerId::ToolsOfTheTrade, 1),
        CardId::Tracking => {
            self_power(c, k, PowerId::Tracking, 50);
        }
        // ── ANCIENT ──
        CardId::Suppress => {
            let t = attack_target(target);
            hit(c, k, t);
            let n = var(c, k, VarKey::Weak, 3);
            c.apply_power_to(t, PowerId::Weak, n, None);
        }
        CardId::WraithForm => {
            let intangible = var(c, k, VarKey::IntangiblePower, 2);
            let wraith = var(c, k, VarKey::WraithFormPower, 1);
            self_power(c, k, PowerId::Intangible, intangible);
            self_power(c, k, PowerId::WraithForm, wraith);
        }
        // ── token ──
        // cards/status.py:1191-1197
        CardId::Shiv => {
            if c.power_amount(who, PowerId::FanOfKnives) > 0 {
                hit_all(c, k);
            } else {
                hit(c, k, attack_target(target));
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod shiv_schema_tests {
    use super::*;
    #[test]
    fn silent_helper_shivs_publish_damage_without_status_factory_mutation_rows() {
        for upgraded in [false,true] {
            let run=crate::run::RunManager::new(1,crate::run::RunOptions::default());
            let mut combat=Combat::new(run.run_ctx().clone(),1);
            shivs(&mut combat,0,3,upgraded);
            let cards:Vec<_>=combat.cards.iter().filter(|c|c.id==CardId::Shiv).collect();
            assert_eq!(cards.len(),3);
            for card in cards {
                assert_eq!(card.damage,Some(if upgraded {6} else {4}));
                assert_eq!(card.var(VarKey::CalcBase),None);
                assert_eq!(card.var(VarKey::CalcExtra),None);
            }
            // Shiv.CanonicalVars is Damage-only for every creation source
            // (41347e8b), not only the Silent helper.
            combat.add_shiv_to_hand(0,upgraded);
            let last=combat.cards.last().unwrap();
            assert_eq!(last.var(VarKey::CalcBase),None);
            assert_eq!(last.var(VarKey::CalcExtra),None);
        }
    }
}

/// Choice continuations.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    let ChoiceSrc::Card(id) = r.src else { return false };
    match (id, r.step) {
        (CardId::Survivor | CardId::Acrobatics | CardId::DaggerThrow | CardId::Prepared, 0) => {
            c.discard_cards(owner(c, r.k), selected, 0);
        }
        (CardId::HiddenDaggers, 0) => {
            c.discard_cards(owner(c, r.k), selected, 0);
            hidden_daggers_finish(c, r.k);
        }
        (CardId::HandTrick, 0) => {
            if let Some(&q) = selected.first() {
                c.cards[q as usize].cv.sly_this_turn = true;
            }
        }
        // cards/silent.py nightmare `_resolver`.
        (CardId::Nightmare, 0) => {
            let Some(&q) = selected.first() else { return true };
            let who = owner(c, r.k);
            let before = c.power_amount(who, PowerId::Nightmare);
            c.apply_power_to(who, PowerId::Nightmare, 3, None);
            if let Some(p) = c.find_power(who, PowerId::Nightmare) {
                let amount = (c.powers[p as usize].amount - before).max(0);
                c.powers[p as usize].amount = amount;
                // set_selected_card: `card.clone(0)` keeps a detached copy; id 0 is falsy, so
                // the clone still draws a fresh instance id.
                let iid = c.run.new_card_id();
                let copy = c.cards[q as usize].clone_with_id(iid);
                let g = c.cards.len() as KRef;
                c.card_hook_mask |= CARD_HOOKS[copy.id.index()];
                c.cards.push(copy);
                c.nightmare_cards.push((who, g, amount));
            }
        }
        // core/combat.py:2479-2504 request_discard (ToolsOfTheTrade).
        (CardId::ToolsOfTheTrade, 1) => c.resolve_request_discard(selected),
        _ => return false,
    }
    true
}

/// cards/silent.py PINPOINT `after_card_entered_combat` hook.
pub(crate) fn on_after_card_entered_combat(c: &mut Combat, listener: KRef, entered: KRef) {
    if listener != entered || c.cards[listener as usize].id != CardId::Pinpoint {
        return;
    }
    let who = owner(c, listener);
    let skills = plays_finished(c, who, CardType::Skill);
    if skills > 0 {
        let card = &mut c.cards[listener as usize];
        // C# Pinpoint.ReduceCostBy.
        card.add_energy_cost(-skills, crate::card::CostExpiry::Turn);
    }
}

/// cards/silent.py PINPOINT `after_card_played` hook.
pub(crate) fn on_after_card_played(c: &mut Combat, listener: KRef, played: KRef) {
    if c.cards[listener as usize].id != CardId::Pinpoint
        || c.cards[played as usize].owner != c.cards[listener as usize].owner
        || c.cards[played as usize].card_type != CardType::Skill {
        return;
    }
    let card = &mut c.cards[listener as usize];
    card.add_energy_cost(-1, crate::card::CostExpiry::Turn);
}

/// GRAND_FINALE playability: only with an empty draw pile.
pub(crate) fn is_playable(c: &Combat, k: KRef) -> Option<bool> {
    match c.cards[k as usize].id {
        CardId::GrandFinale => Some(c.players[owner(c, k)].draw.is_empty()),
        _ => None,
    }
}

/// SHIV target type: all enemies under Fan of Knives (cards/status.py:1200-1204).
pub(crate) fn target_type(c: &Combat, k: KRef) -> Option<TargetType> {
    let card = &c.cards[k as usize];
    match card.id {
        CardId::Shiv if c.power_amount(card.owner, PowerId::FanOfKnives) > 0 => Some(TargetType::AllEnemies),
        _ => None,
    }
}
