//! r7 `cards/ironclad.py`: Ironclad attack cards (work-session packet; see docs/packets/).

use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, MAX_HAND_SIZE, PileKind, PilePos, Props};
use crate::tables::*;
use arrayvec::ArrayVec;

#[inline]
fn attack_target(target: Option<CId>) -> CId {
    target.expect("ANY_ENEMY card without target")
}

#[inline]
fn ironclad_dynamic_damage(c: &mut Combat, k: KRef, target: CId, base: i32) {
    let owner = c.cards[k as usize].owner;
    if c.cr(owner).is_dead() {
        return;
    }
    let damage = c.calculate_damage(base, Some(owner), target, Props::MOVE);
    c.apply_damage(target, damage, Props::MOVE, Some(owner));
}

#[inline]
fn ironclad_damage_all(c: &mut Combat, k: KRef) {
    let owner = c.cards[k as usize].owner;
    if c.cr(owner).is_dead() {
        return;
    }
    c.deal_card_damage_all(k);
}

#[inline]
fn ironclad_self_hp_loss(c: &mut Combat, k: KRef, amount: i32) {
    let owner = c.cards[k as usize].owner;
    let props = Props::UNBLOCKABLE.or(Props::UNPOWERED).or(Props::MOVE);
    c.apply_damage(owner, amount, props, Some(owner));
}

fn ironclad_exhaust_hand_card(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let who = c.cards[k as usize].owner;
    let Some(pos) = c.players[who].hand.iter().position(|&x| x == k) else {
        return;
    };
    c.players[who].hand.remove(pos);
    c.players[who].exhaust.push(k);
    c.fire_after_card_changed_piles(k, crate::combat::PileKind::Hand, None);
    c.fire_after_card_exhausted(k);
}

fn ironclad_increase_damage(c: &mut Combat, k: KRef, amount: i32) {
    let card = &mut c.cards[k as usize];
    card.damage = Some(card.damage.unwrap_or(0) + amount);
    if let Some((_, value)) = card.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
        *value += amount;
    }
}

fn ironclad_untargeted_card_damage(c: &mut Combat, k: KRef, owner: CId) -> Option<i32> {
    if !c.cards[k as usize].enchantments.is_empty() {
        // Enchantments are handled by the shared targetless modifier pipeline below.
    }
    let base = c.cards[k as usize].damage.unwrap_or(0);
    Some(c.modify_untargeted_card_damage(base, owner, k))
}

fn ironclad_gain_max_hp(c: &mut Combat, owner: CId, amount: i32) {
    if amount <= 0 || c.cr(owner).is_dead() {
        return;
    }
    c.cr_mut(owner).max_hp += amount;
    c.heal(owner, amount);
    c.run.max_hp = c.cr(owner).max_hp;
    c.run.hp = c.cr(owner).hp;
}

fn ironclad_feed_fatal_allowed(c: &mut Combat, target: CId) -> bool {
    c.should_owner_death_trigger_fatal(target)
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, target: Option<CId>) -> bool {
    let who = c.cards[k as usize].owner;
    match c.cards[k as usize].id {
        CardId::Outrage => {
            // C# Outrage.OnPlay: attack, then clone for each living teammate's discard.
            c.deal_card_damage(k, attack_target(target));
            for player in 0..c.players.len() as CId {
                if c.cr(player).is_alive() {
                    let iid = c.run.new_card_id();
                    let mut clone = c.cards[k as usize].clone_with_id(iid);
                    clone.owner = player;
                    let generated = c.cards.len() as KRef;
                    c.card_hook_mask |= CARD_HOOKS[clone.id.index()];
                    c.cards.push(clone);
                    c.add_generated_card(generated, PileKind::Discard, PilePos::Bottom, true);
                }
            }
        }
        CardId::Anger => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let (upgraded, cost, damage) = {
                let card = &c.cards[k as usize];
                (card.upgraded, card.cost, card.damage)
            };
            // r7 builds the copy (consuming an instance id) before add_card_to_discard, which
            // itself returns early once the combat is ending.
            if let Some(copy) = c.new_card(who, CardId::Anger, upgraded) {
                let card = &mut c.cards[copy as usize];
                card.set_base_energy_cost(cost);
                card.damage = damage;
                // r7 constructs this CardInstance directly rather than through
                // make_anger(), so its effect_vars dict is empty.
                card.vars.clear();
                c.add_generated_card(copy, PileKind::Discard, PilePos::Bottom, true);
            }
        }
        CardId::BodySlam => {
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            ironclad_dynamic_damage(c, k, target, c.cr(owner).block);
        }
        CardId::Breakthrough => {
            let hp_loss = c.cards[k as usize].var(VarKey::HpLoss).unwrap_or(1);
            ironclad_self_hp_loss(c, k, hp_loss);
            ironclad_damage_all(c, k);
        }
        CardId::Cinder => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            if !c.players[who].hand.is_empty() {
                let index = c.run.rngs.combat_card_selection.choice_index(c.players[who].hand.len());
                let exhausted = c.players[who].hand[index];
                ironclad_exhaust_hand_card(c, exhausted);
            }
        }
        CardId::Headbutt => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            if c.is_over_or_ending() {
                return true;
            }
            match c.players[who].discard.len() {
                0 => {}
                1 => {
                    let selected = c.players[who].discard.remove(0);
                    c.insert_pile_from(selected, PileKind::Discard, PileKind::Draw, PilePos::Top);
                    c.fire_after_card_changed_piles(selected, PileKind::Discard, None);
                }
                _ => {
                    let candidates = c.players[who].discard.clone();
                    let r = Resolver {
                        src: ChoiceSrc::Card(CardId::Headbutt),
                        k,
                        target: Some(target),
                        step: 0,
                        arg: 0,
                    };
                    c.request_card_choice(&candidates, PileKind::Discard, r, false);
                }
            }
        }
        CardId::IronWave => {
            let target = attack_target(target);
            c.gain_card_block(k);
            c.deal_card_damage(k, target);
        }
        CardId::MoltenFist => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            if c.cr(target).is_alive() {
                let vulnerable = c.power_amount(target, PowerId::Vulnerable);
                if vulnerable > 0 {
                    c.apply_power_to(target, PowerId::Vulnerable, vulnerable, None);
                }
            }
        }
        CardId::PerfectedStrike => {
            let target = attack_target(target);
            let card = &c.cards[k as usize];
            let base = card.var(VarKey::CalcBase).unwrap_or(6);
            let extra = card.var(VarKey::ExtraDamage).unwrap_or(2);
            let strikes = c.players[who]
                .hand
                .iter()
                .chain(c.players[who].draw.iter())
                .chain(c.players[who].discard.iter())
                .chain(c.players[who].exhaust.iter())
                .chain(c.players[who].play.iter())
                .filter(|&&q| c.cards[q as usize].has_tag(CardTag::Strike))
                .count() as i32;
            ironclad_dynamic_damage(c, k, target, base + extra * strikes);
        }
        CardId::PommelStrike => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let owner = c.cards[k as usize].owner;
            let cards = c.cards[k as usize].var(VarKey::Cards).unwrap_or(1);
            c.draw_cards(owner, cards, false);
        }
        CardId::SetupStrikeCard => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let owner = c.cards[k as usize].owner;
            let strength = c.cards[k as usize].var(VarKey::Strength).unwrap_or(2);
            c.apply_power_to(owner, PowerId::SetupStrike, strength, None);
        }
        CardId::SwordBoomerang => {
            let owner = c.cards[k as usize].owner;
            let hits = c.cards[k as usize].var(VarKey::Repeat).unwrap_or(3);
            for _ in 0..hits {
                if c.cr(owner).is_dead() {
                    break;
                }
                let alive = c.hittable_enemies();
                if alive.is_empty() {
                    break;
                }
                let index = c.run.rngs.combat_targets.choice_index(alive.len());
                c.deal_card_damage(k, alive[index]);
            }
        }
        CardId::Thunderclap => {
            let vulnerable = c.cards[k as usize].var(VarKey::Vulnerable).unwrap_or(1);
            ironclad_damage_all(c,k);
            for enemy in c.hittable_enemies() {
                c.apply_power_to(enemy, PowerId::Vulnerable, vulnerable, None);
            }
        }
        CardId::TwinStrike => {
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            for _ in 0..2 {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                c.deal_card_damage(k, target);
            }
        }
        CardId::AshenStrike => {
            let target = attack_target(target);
            let card = &c.cards[k as usize];
            let base = card.var(VarKey::CalcBase).unwrap_or(6);
            let extra = card.var(VarKey::ExtraDamage).unwrap_or(3);
            ironclad_dynamic_damage(c, k, target, base + extra * c.players[who].exhaust.len() as i32);
        }
        CardId::Bludgeon => {
            c.deal_card_damage(k, attack_target(target));
        }
        CardId::Bully => {
            let target = attack_target(target);
            let card = &c.cards[k as usize];
            let base = card.var(VarKey::CalcBase).unwrap_or(4);
            let extra = card.var(VarKey::ExtraDamage).unwrap_or(2);
            let vulnerable = c.power_amount(target, PowerId::Vulnerable);
            ironclad_dynamic_damage(c, k, target, base + extra * vulnerable);
        }
        CardId::Dismantle => {
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            let hits = if c.has_power(target, PowerId::Vulnerable) {
                2
            } else {
                1
            };
            for _ in 0..hits {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                c.deal_card_damage(k, target);
            }
        }
        CardId::FightMe => {
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            let hits = c.cards[k as usize].var(VarKey::Repeat).unwrap_or(2);
            for _ in 0..hits {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                c.deal_card_damage(k, target);
            }
            let strength = c.cards[k as usize].var(VarKey::Strength).unwrap_or(2);
            c.apply_power_to(owner, PowerId::Strength, strength, None);
            if c.cr(target).is_alive() {
                let enemy_strength = c.cards[k as usize].var(VarKey::EnemyStrength).unwrap_or(1);
                c.apply_power_to(target, PowerId::Strength, enemy_strength, None);
            }
        }
        CardId::Grapple => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let amount = c.cards[k as usize].var(VarKey::GrapplePower).unwrap_or(5);
            c.apply_power_to(target, PowerId::Grapple, amount, None);
        }
        CardId::Hemokinesis => {
            let target = attack_target(target);
            let hp_loss = c.cards[k as usize].var(VarKey::HpLoss).unwrap_or(2);
            ironclad_self_hp_loss(c, k, hp_loss);
            c.deal_card_damage(k, target);
        }
        CardId::HowlFromBeyond => {
            ironclad_damage_all(c, k);
        }
        CardId::Pillage => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let owner = c.cards[k as usize].owner;
            // C# Pillage: draw while the drawn card is an Attack (a Strike Hellraiser already
            // auto-played still counts) and the hand is not full.
            while c.players[who].hand.len() < MAX_HAND_SIZE {
                match c.draw_cards(owner, 1, false) {
                    Some(drawn) if c.cards[drawn as usize].card_type == CardType::Attack => {}
                    _ => break,
                }
            }
        }
        CardId::Rampage => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let increase = c.cards[k as usize].var(VarKey::Increase).unwrap_or(5);
            ironclad_increase_damage(c, k, increase);
        }
        CardId::Spite => {
            // r7 cards/ironclad.py:1479-1492
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            let hits = if c.has_unblocked_damage_received_this_turn(owner, Some(crate::creature::Side::Player)) {
                c.cards[k as usize].var(VarKey::Repeat).unwrap_or(2)
            } else {
                1
            };
            for _ in 0..hits.max(0) {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                c.deal_card_damage(k, target);
            }
        }
        CardId::Stomp => {
            ironclad_damage_all(c, k);
        }
        CardId::Unrelenting => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let owner = c.cards[k as usize].owner;
            c.apply_power_to(owner, PowerId::FreeAttack, 1, None);
        }
        CardId::Uppercut => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let amount = c.cards[k as usize].var(VarKey::Power).unwrap_or(1);
            c.apply_power_to(target, PowerId::Weak, amount, None);
            c.apply_power_to(target, PowerId::Vulnerable, amount, None);
        }
        CardId::Whirlwind => {
            let owner = c.cards[k as usize].owner;
            let x = c.cards[k as usize].energy_spent;
            for _ in 0..x {
                if c.cr(owner).is_dead() {
                    break;
                }
                c.deal_card_damage_all(k);
            }
        }
        CardId::Conflagration => {
            let owner = c.cards[k as usize].owner;
            let repeats = c.cards[k as usize].var(VarKey::Repeat).unwrap_or(4);
            for _ in 0..repeats {
                if c.cr(owner).is_dead() || c.is_over {
                    break;
                }
                c.deal_card_damage_all(k);
            }
        }
        CardId::Feed => {
            let target = attack_target(target);
            let fatal = ironclad_feed_fatal_allowed(c, target);
            c.deal_card_damage(k, target);
            if fatal && c.cr(target).is_dead() {
                let amount = c.cards[k as usize].var(VarKey::MaxHp).unwrap_or(3);
                let owner = c.cards[k as usize].owner;
                ironclad_gain_max_hp(c, owner, amount);
            }
        }
        CardId::FiendFire => {
            let target = attack_target(target);
            c.start_exhaust_caller(k, Some(target));
        }
        CardId::Mangle => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let amount = c.cards[k as usize].var(VarKey::StrengthLoss).unwrap_or(10);
            c.apply_power_to(target, PowerId::Mangle, amount, None);
        }
        CardId::PactsEnd => {
            let threshold = c.cards[k as usize].var(VarKey::Cards).unwrap_or(3);
            if c.players[who].exhaust.len() as i32 >= threshold {
                ironclad_damage_all(c, k);
            }
        }
        CardId::TearAsunder => {
            // r7 cards/ironclad.py:2290-2299
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            let hits = 1 + c.hist.unblocked_hits_received_combat(owner);
            for _ in 0..hits {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                let base = c.cards[k as usize].damage.unwrap_or(0);
                let dmg = c.calculate_damage(base, Some(owner), target, Props::MOVE);
                c.apply_damage(target, dmg, Props::MOVE, Some(owner));
            }
        }
        CardId::Thrash => {
            let target = attack_target(target);
            let owner = c.cards[k as usize].owner;
            for _ in 0..2 {
                if c.cr(owner).is_dead() || c.cr(target).is_dead() {
                    break;
                }
                c.deal_card_damage(k, target);
            }
            let attacks: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who]
                .hand
                .iter()
                .copied()
                .filter(|&q| c.cards[q as usize].card_type == CardType::Attack)
                .collect();
            if !attacks.is_empty() {
                let index = c.run.rngs.combat_card_selection.choice_index(attacks.len());
                let exhausted = attacks[index];
                let bonus = ironclad_untargeted_card_damage(c, exhausted, owner);
                #[cfg(feature = "selector-proof")]
                let receipt = bonus.and_then(|bonus| {
                    // PenNib.cs: without AttackToDouble, a non-Play source at counter
                    // nine doubles even in Thrash's targetless growth calculation.
                    let r=c.run.relics.iter().position(|r|r.id==crate::tables::RelicId::PenNib
                        && r.owner==owner && r.counter==9 && r.card.is_none())?;
                    if exhausted==k || c.card_pile(exhausted)!=crate::combat::PileKind::Hand
                        || c.fault.is_some() {return None;}
                    let mut legacy=c.clone();
                    // Neutralize only this ninth-attack preview, preserving every
                    // other modifier in the shared targetless damage pipeline.
                    legacy.run.relics[r].counter=0;
                    let legacy_bonus=ironclad_untargeted_card_damage(&mut legacy,exhausted,owner)?;
                    if legacy.fault.is_some() || bonus==legacy_bonus {return None;}
                    let before=c.cards[k as usize].damage?;
                    Some((c.cards[k as usize].instance_id,c.cards[exhausted as usize].instance_id,
                        owner,before,before+bonus,before+legacy_bonus))
                });
                if let Some(bonus) = bonus {
                    ironclad_increase_damage(c, k, bonus);
                }
                ironclad_exhaust_hand_card(c, exhausted);
                #[cfg(feature = "selector-proof")]
                if let Some(receipt)=receipt {
                    if c.players[owner].exhaust.contains(&exhausted) {
                        c.thrash_pen_nib_proof.push(receipt);
                    }
                }
            }
        }
        CardId::Break => {
            let target = attack_target(target);
            c.deal_card_damage(k, target);
            let vulnerable = c.cards[k as usize].var(VarKey::Vulnerable).unwrap_or(5);
            c.apply_power_to(target, PowerId::Vulnerable, vulnerable, None);
        }
        _ => return false,
    }
    true
}

/// r7 cards/ironclad.py:1534-1545 `stomp_after_card_entered_combat`.
pub(crate) fn on_after_card_entered_combat(c: &mut Combat, listener: KRef, entered: KRef) {
    if listener != entered || c.cards[listener as usize].id != CardId::Stomp {
        return;
    }
    let attacks = c
        .hist
        .played_this_turn
        .iter()
        // Stomp.cs AfterCardEnteredCombat: immutable CardPlay.Player == Owner.
        .filter(|&&(actor, k)| actor == c.cards[listener as usize].owner
            && c.cards[k as usize].card_type == CardType::Attack)
        .count() as i32;
    if attacks > 0 {
        let card = &mut c.cards[listener as usize];
        // C# Stomp.ReduceCostBy: ordered relative AddThisTurn.
        card.add_energy_cost(-attacks, crate::card::CostExpiry::Turn);
    }
}

/// r7 cards/ironclad.py:1548-1556 `stomp_before_card_played`.
pub(crate) fn on_before_card_played(c: &mut Combat, listener: KRef, played: KRef) {
    if c.cards[listener as usize].id != CardId::Stomp
        || c.cards[played as usize].card_type != CardType::Attack
        // Stomp.cs BeforeCardPlayed explicitly excludes teammates' attacks.
        || c.cards[played as usize].owner != c.cards[listener as usize].owner
    {
        return;
    }
    let card = &mut c.cards[listener as usize];
    card.add_energy_cost(-1, crate::card::CostExpiry::Turn);
}

/// C# HowlFromBeyond.cs:32-38 awaits autoplay from Exhaust during AutoPostPlay.
pub(crate) fn on_after_auto_post_play(c: &mut Combat, listener: KRef, player: CId) {
    if c.cards[listener as usize].id != CardId::HowlFromBeyond
        || c.cards[listener as usize].owner != player
        || !c.players[player].exhaust.contains(&listener)
    {
        return;
    }
    c.auto_play_card(listener, None, false);
}

/// Choice continuations for Ironclad attacks.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    match r.src {
        ChoiceSrc::Card(CardId::Headbutt) => {
            if let Some(&picked) = selected.first()
                && let Some(i) = c.players[c.cards[picked as usize].owner].discard.iter().position(|&q| q == picked)
            {
                c.players[c.cards[picked as usize].owner].discard.remove(i);
                c.insert_pile_from(picked, PileKind::Discard, PileKind::Draw, PilePos::Top);
                c.fire_after_card_changed_piles(picked, PileKind::Discard, None);
            }
        }
        _ => return false,
    }
    true
}
