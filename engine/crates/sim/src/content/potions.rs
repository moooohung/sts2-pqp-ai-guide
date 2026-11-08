//! r7 potions/effects.py: combat potion effects available to an Ironclad run.
//! Character-specific potions for other characters are intentionally outside this packet.
use crate::character::Character;

use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, Resolver};
use crate::combat::{Combat, PileKind, Props, MAX_HAND_SIZE, PLAYER};
use crate::hooks::Ev;
use crate::tables::*;

#[inline]
fn potion_resolver(id: PotionId, player: CId, step: u8) -> Resolver {
    Resolver {
        src: ChoiceSrc::Potion(id),
        k: player as KRef,
        target: None,
        step,
        arg: 0,
    }
}

fn gain_energy_for_player(c: &mut Combat, player: CId, amount: i32) {
    if amount > 0 && !c.is_over_or_ending() {
        c.players[player].energy += amount;
    }
}

fn gain_unpowered_block(c: &mut Combat, target: CId, amount: i32) {
    let before = c.cr(target).block;
    c.gain_recorded_block(target,amount);
    if c.cr(target).block > before {
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

fn deal_unpowered_damage(c: &mut Combat, owner: CId, target: CId, amount: i32) {
    let damage = c.calculate_damage(amount, Some(owner), target, Props::UNPOWERED);
    c.apply_damage(target, damage, Props::UNPOWERED, Some(owner));
}

fn move_to_hand(c: &mut Combat, k: KRef) {
    let old = c.card_pile(k);
    c.remove_card_from_piles(k);
    let owner = c.cards[k as usize].owner;
    if c.players[owner].hand.len() < MAX_HAND_SIZE {
        c.players[owner].hand.push(k);
    } else {
        c.players[owner].discard.push(k);
    }
    if old != c.card_pile(k) { c.fire_after_card_changed_piles(k, old, None); }
}

fn exhaust_from_hand(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let owner = c.cards[k as usize].owner;
    let Some(i) = c.players[owner].hand.iter().position(|&x| x == k) else {
        return;
    };
    c.players[owner].hand.remove(i);
    c.players[owner].exhaust.push(k);
    c.fire_after_card_changed_piles(k, PileKind::Hand, None);
    c.fire_after_card_exhausted(k);
}

// upgrade_card: the engine `Combat::upgrade_card` (consumes the reference + template ids like r7).


pub(super) fn gain_max_hp(c: &mut Combat, player: CId, amount: i32) {
    if amount <= 0 {
        return;
    }
    let before = c.cr(player).hp;
    {
        let p = c.cr_mut(player);
        p.max_hp += amount;
        p.hp = (p.hp + amount).min(p.max_hp);
    }
    if c.cr(player).hp != before && !c.is_over {
        c.fire(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: player, delta: c.cr(player).hp - before }, None);
    }
    if player == PLAYER {
        c.run.max_hp += amount;
        c.run.hp = (c.run.hp + amount).min(c.run.max_hp);
    }
}

/// Alchemize.OnPlay -> PotionFactory.CreateRandomPotionInCombat -> PotionCmd.TryToProcure.
fn random_potion(c: &mut Combat, owner: CId, in_combat: bool) -> Option<PotionId> {
    // PotionFactory.cs:64-66 uses GetUnlockedPotions for BOTH factories.
    // Epoch state is not modelled: explicit full-unlock inputs are exact;
    // retain the historical r7 solo pool otherwise, never guess MP epochs.
    let legacy_solo = c.run.all_cards_unlocked != Some(true);
    if legacy_solo && c.players.len() != 1 {
        c.unported("potion generation requires explicit all-unlocked multiplayer configuration");
        return None;
    }
    // The legacy solo run protocol is Ironclad (Combat::new). Missing MP
    // character identities still refuse; never infer them from a mixed deck.
    let character = c.players[owner].character.or_else(||
        (owner == PLAYER && c.players.len() == 1).then_some(Character::Ironclad));
    let Some(character) = character else {
        c.unported("Alchemize requires owner character");
        return None;
    };
    let pool = if legacy_solo {
        let character = match character {
            Character::Ironclad => "Ironclad", Character::Silent => "Silent", Character::Defect => "Defect",
            Character::Regent => "Regent", Character::Necrobinder => "Necrobinder",
            _ => { c.unported("Alchemize unsupported owner potion pool"); return None; }
        };
        let eligible = |id: PotionId| {
            let info = &POTION_INFO[id.index()];
            matches!(info.rarity, PotionRarity::Common | PotionRarity::Uncommon | PotionRarity::Rare)
                && (!info.character_specific || info.character_pool == Some(character))
        };
        Some(PotionId::ALL.into_iter().filter(|&id| eligible(id) && POTION_INFO[id.index()].character_specific)
            .chain(PotionId::ALL.into_iter().filter(|&id| eligible(id) && !POTION_INFO[id.index()].character_specific))
            .collect::<Vec<_>>())
    } else { crate::run::potion_pools::cards(character) };
    let Some(pool) = pool else {
        c.unported("Alchemize unsupported owner potion pool");
        return None;
    };
    if owner != PLAYER && c.players[owner].potions.is_none() {
        c.unported("Alchemize requires owner potion slots");
        return None;
    }
    // The factory consumes both rolls even when procurement is denied or slots are full.
    let roll = c.run.rngs.combat_potion_generation.next_float(1.0);
    let rarity = if roll <= 0.1 { PotionRarity::Rare }
        else if roll <= 0.35 { PotionRarity::Uncommon } else { PotionRarity::Common };
    let options: Vec<_> = pool.into_iter().filter(|id| {
        let info = &POTION_INFO[id.index()];
        (!in_combat || info.gen_combat) && info.rarity == rarity
    }).collect();
    // Original callers dereference Rng.NextItem(empty); refuse rather than
    // silently skipping the hook or indexing an empty vector.
    if options.is_empty() {
        c.unported("potion generation has no candidates for selected rarity");
        return None;
    }
    let id = options[c.run.rngs.combat_potion_generation.choice_index(options.len())];
    Some(id)
}

pub(super) fn alchemize(c: &mut Combat, owner: CId) {
    let Some(id) = random_potion(c, owner, true) else { return };
    procure_potion(c, owner, id);
}

fn procure_potion(c: &mut Combat, owner: CId, id: PotionId) -> bool {
    // Sozu.ShouldProcurePotion denies only its owner.
    if c.run.relics.iter().any(|r| r.enabled && r.owner == owner && r.id == RelicId::Sozu) {
        return false;
    }
    let success = if owner == PLAYER { c.add_potion(id) } else {
        let slots = c.players[owner].potions.as_mut().unwrap();
        if let Some(slot) = slots.iter_mut().find(|p| p.is_none()) {
            *slot = Some(id);
            true
        } else { false }
    };
    if success {
        // The owner-aware lifecycle helper filters other seats and retains the
        // admission refusal for an unaudited nonzero-seat Belt Buckle.
        c.fire_relic_after_potion_procured(owner);
    }
    success
}

pub(super) fn fill_empty_potion_slots_out_of_combat_pool(c: &mut Combat, owner: CId) {
    // DelicateFrond.cs:12-21 uses the same factory as Alchemize, but without
    // the in-combat generation restriction. TryToProcure notifies listeners.
    // Solo snapshots store occupied potions compactly, unlike explicit MP slots.
    if owner == PLAYER && c.run.potions.len() < c.run.max_potion_slots.max(0) as usize {
        c.run.potions.resize(c.run.max_potion_slots.max(0) as usize, None);
    }
    loop {
        let max_slots = if owner == PLAYER { c.run.max_potion_slots.max(0) as usize }
            else { c.players[owner].potions.as_ref().map_or(0, Vec::len) };
        let Some(slots) = c.relic_owner_potions(owner) else { return };
        if !slots.iter().take(max_slots).any(Option::is_none) { return; }
        let Some(id) = random_potion(c, owner, false) else { return };
        if !procure_potion(c, owner, id) { return; }
    }
}

pub(crate) fn use_effect(c: &mut Combat, owner: CId, id: PotionId, target: Option<CId>) -> bool {
    let player = target.unwrap_or(owner);
    // Models/Potions/*.OnUse: Owner is the applier/dealer; target.Player
    // selects the resource/hand recipient independently.
    let player_target = matches!(POTION_INFO[id.index()].target,
        PotionTargetType::AnyPlayer | PotionTargetType::Self_);
    if !c.players.is_player(owner) || (player_target && !c.players.is_player(player)) {
        c.unported("potion selector requires a valid player");
        return true;
    }
    match id {
        // C# BoneBrew/FocusPotion/PoisonPotion/StarPotion.OnUse.
        PotionId::Bonebrew => { c.summon_osty(player, 15); }
        PotionId::Focuspotion => { c.apply_power_to(player, PowerId::Focus, 2, Some(player)); }
        PotionId::Poisonpotion => {
            if let Some(t) = target { c.apply_power_to(t, PowerId::Poison, 6, Some(owner)); }
        }
        PotionId::Starpotion => { c.gain_stars(player, 3); }
        // DeprecatedPotion inherits PotionModel.OnUse (completed no-op).
        PotionId::Deprecatedpotion => {}
        PotionId::Ghostinajar => { c.apply_power_to(player, PowerId::Intangible, 1, Some(owner)); }
        PotionId::Kingscourage => { c.forge(player, 15, None); }
        PotionId::Potionofdoom => {
            if let Some(t) = target { c.apply_power_to(t, PowerId::Doom, 33, Some(owner)); }
        }
        PotionId::Cosmicconcoction => {
            // CosmicConcoction.OnUse / CardFactory.GetDistinctForCombat:
            // target.Player owns the created cards; Potion.Owner is their creator.
            if c.run.all_cards_unlocked != Some(true) {
                c.unported("Cosmic Concoction requires explicit all-unlocked run configuration");
                return true;
            }
            for g in c.generate_unlocked_colorless_cards(player, 3) {
                c.upgrade_card(g);
                c.add_generated_card(g, PileKind::Hand, crate::combat::PilePos::Bottom, true);
            }
        }
        PotionId::Cunningpotion | PotionId::Potofghouls => {
            // Shiv/Soul.CreateInHand retain the target player's ownership;
            // CardPileCmd.AddGeneratedCardsToCombat's creator is not a transferee.
            let (card, count) = if id == PotionId::Cunningpotion {
                (CardId::Shiv, 3)
            } else { (CardId::Soul, 2) };
            let mut generated = Vec::new();
            for _ in 0..count {
                if let Some(k) = c.new_card(player, card, false) { generated.push(k); }
            }
            for &k in &generated {
                c.add_generated_card(k, PileKind::Hand, crate::combat::PilePos::Bottom, true);
            }
            if id == PotionId::Cunningpotion {
                for k in generated { c.upgrade_card(k); }
            }
        }
        PotionId::Essenceofdarkness => {
            // Capture capacity before channeling; evocations may alter state.
            let count = c.players[player].orbs.capacity;
            for _ in 0..count { c.channel_orb(player, crate::orbs::OrbKind::Dark); }
        }
        PotionId::Potionofcapacity => c.add_orb_slots(player, 2),
        PotionId::Attackpotion | PotionId::Powerpotion | PotionId::Skillpotion => {
            // r7 potions/effects.py _attack/_power/_skill_potion: 3 distinct class cards of the
            // type, all free this turn, choose one (skippable) into hand.
            let ct = match id {
                PotionId::Attackpotion => CardType::Attack,
                PotionId::Powerpotion => CardType::Power,
                _ => CardType::Skill,
            };
            let generated = c.generate_unlocked_owner_cards(player, Some(ct), 3);
            for &g in &generated {
                c.set_temporary_free_this_turn(g);
            }
            if !generated.is_empty() {
                let mut resolver = potion_resolver(id, player, 0);
                resolver.target = Some(owner);
                c.request_card_choice_for_player(player, &generated, PileKind::None, resolver, true);
            }
        }
        PotionId::Colorlesspotion => {
            let generated = c.generate_unlocked_colorless_cards(player, 3);
            for &g in &generated {
                c.set_temporary_free_this_turn(g);
            }
            if !generated.is_empty() {
                let mut resolver = potion_resolver(id, player, 0);
                resolver.target = Some(owner);
                c.request_card_choice_for_player(player, &generated, PileKind::None, resolver, true);
            }
        }
        PotionId::Blockpotion => gain_unpowered_block(c, player, 12),
        PotionId::Bloodpotion => {
            let t = player;
            c.heal(t, c.cr(t).max_hp * 20 / 100);
        }
        PotionId::Dexteritypotion => {
            c.apply_power_to(
                player,
                PowerId::Dexterity,
                2,
                Some(owner),
            );
        }
        PotionId::Energypotion => gain_energy_for_player(c, player, 2),
        PotionId::Explosiveampoule => {
            for e in c.hittable_enemies() {
                deal_unpowered_damage(c, owner, e, 10);
            }
        }
        PotionId::Firepotion => {
            if let Some(t) = target {
                deal_unpowered_damage(c, owner, t, 20);
            }
        }
        PotionId::Flexpotion => {
            c.apply_power_to(
                player,
                PowerId::FlexPotion,
                5,
                Some(owner),
            );
        }
        PotionId::Speedpotion => {
            c.apply_power_to(
                player,
                PowerId::SpeedPotion,
                5,
                Some(owner),
            );
        }
        PotionId::Strengthpotion => {
            c.apply_power_to(player, PowerId::Strength, 2, Some(owner));
        }
        PotionId::Swiftpotion => {
            c.draw_cards(player, 3, false);
        }
        PotionId::Vulnerablepotion => {
            if let Some(t) = target {
                c.apply_power_to(t, PowerId::Vulnerable, 3, Some(owner));
            }
        }
        PotionId::Weakpotion => {
            if let Some(t) = target {
                c.apply_power_to(t, PowerId::Weak, 3, Some(owner));
            }
        }

        PotionId::Ashwater => {
            if !c.players[player].hand.is_empty() {
                let cards: Vec<KRef> = c.players[player].hand.iter().copied().collect();
                c.request_multi_card_choice_for_player(player,
                    &cards,
                    PileKind::Hand,
                    potion_resolver(id, player, 1),
                    0,
                    Some(cards.len() as u16),
                    true,
                );
            }
        }
        PotionId::Blessingoftheforge => {
            let hand: Vec<KRef> = c.players[player].hand.iter().copied().collect();
            for k in hand {
                c.upgrade_card(k);
            }
        }
        PotionId::Clarity => {
            let t = player;
            c.draw_cards(t, 1, false);
            c.apply_power_to(t, PowerId::Clarity, 3, Some(owner));
        }
        PotionId::Cureall => {
            gain_energy_for_player(c, player, 1);
            c.draw_cards(player, 2, false);
        }
        PotionId::Duplicator => {
            // Duplicator.OnUse uses the recipient itself as applier.
            c.apply_power_to(player, PowerId::Duplication, 1, Some(player));
        }
        PotionId::Fortifier => {
            let t = player;
            gain_unpowered_block(c, t, c.cr(t).block * 2);
        }
        PotionId::Fyshoil => {
            let t = player;
            c.apply_power_to(t, PowerId::Strength, 1, Some(owner));
            c.apply_power_to(t, PowerId::Dexterity, 1, Some(owner));
        }
        PotionId::Gamblersbrew => {
            // C# GamblersBrew.OnUse selects the target player's hand.
            let who = player;
            if !c.players[who].hand.is_empty() {
                let cards: Vec<KRef> = c.players[who].hand.iter().copied().collect();
                let mut resolver = potion_resolver(id, who, 2);
                resolver.target = Some(who);
                c.request_multi_card_choice_for_player(who,
                    &cards,
                    PileKind::Hand,
                    resolver,
                    0,
                    Some(cards.len() as u16),
                    true,
                );
            }
        }
        PotionId::Heartofiron => {
            c.apply_power_to(player, PowerId::Plating, 7, Some(owner));
        }
        PotionId::Liquidbronze => {
            c.apply_power_to(player, PowerId::Thorns, 3, Some(owner));
        }
        PotionId::Potionofbinding => {
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::Weak, 1, Some(owner));
                c.apply_power_to(e, PowerId::Vulnerable, 1, Some(owner));
            }
        }
        PotionId::Powdereddemise => {
            if let Some(t) = target {
                c.apply_power_to(t, PowerId::Demise, 9, Some(owner));
            }
        }
        PotionId::Radianttincture => {
            let t = player;
            gain_energy_for_player(c, t, 1);
            c.apply_power_to(t, PowerId::Radiance, 3, Some(owner));
        }
        PotionId::Regenpotion => {
            c.apply_power_to(player, PowerId::Regen, 5, Some(owner));
        }
        PotionId::Stableserum => {
            c.apply_power_to(
                player,
                PowerId::RetainHand,
                2,
                Some(owner),
            );
        }
        PotionId::Touchofinsanity => {
            let mut cards = Vec::new();
            let hand: Vec<KRef> = c.players[player].hand.iter().copied().collect();
            for k in hand {
                let (x_cost, cost, star_x, star_cost) = {
                    let card = &c.cards[k as usize];
                    (card.x_cost, card.cost, card.star_x, card.star_cost)
                };
                let energy_candidate = !x_cost && cost > 0;
                let star_candidate = !star_x && star_cost > 0;
                if energy_candidate
                    || star_candidate
                    || (!x_cost && c.modified_card_cost(player, k) > 0)
                    || (!star_x && c.modified_star_cost(k) > 0)
                {
                    cards.push(k);
                }
            }
            if !cards.is_empty() {
                c.request_card_choice_for_player(player, &cards, PileKind::Hand, potion_resolver(id, player, 3), false);
            }
        }

        PotionId::Beetlejuice => {
            if let Some(t) = target {
                c.apply_power_to(t, PowerId::Shrink, 4, Some(owner));
            }
        }
        PotionId::Bottledpotential => {
            let hand: Vec<KRef> = c.players[player].hand.drain(..).collect();
            c.players[player].draw.extend(hand.iter().copied());
            let discarded = c.players[player].discard.clone();
            let p = &mut c.players[player];
            p.draw.append(&mut p.discard);
            for q in hand { c.fire_after_card_changed_piles(q, PileKind::Hand, None); }
            for q in discarded { c.fire_after_card_changed_piles(q, PileKind::Discard, None); }
            c.players[player].draw.sort_by_key(|&k| {
                (
                    c.cards[k as usize].id.name(),
                    c.cards[k as usize].upgraded as u8,
                )
            });
            c.run.rngs.shuffle.shuffle(&mut c.players[player].draw);
            c.draw_cards(player, 5, false);
        }
        // DistilledChaos.OnUse awaits AutoPlayFromDrawPile for target.Player,
        // defaulting to the potion owner when no teammate target is supplied.
        PotionId::Distilledchaos => c.auto_play_from_draw(player, 3, false),
        PotionId::Dropletofprecognition => {
            if !c.players[player].draw.is_empty() {
                let cards = super::kit::draw_selector_order(c, player);
                c.request_card_choice_for_player(player, &cards, PileKind::Draw, potion_resolver(id, player, 4), false);
            }
        }
        PotionId::Entropicbrew => {
            // EntropicBrew.OnUse uses targetPlayer's unlocked out-of-combat pool.
            // Player.HasOpenPotionSlots scans fixed slots, not the lazy run Vec.
            let capacity = if player == PLAYER {
                let capacity = c.run.max_potion_slots.max(0) as usize;
                if c.run.potions.len() < capacity { c.run.potions.resize(capacity, None); }
                capacity
            } else {
                let Some(slots) = c.potion_slots_for_player(player) else {
                    c.unported("Entropic Brew requires target potion slots");
                    return true;
                };
                slots.len()
            };
            loop {
                let Some(slots) = c.potion_slots_for_player(player) else {
                    c.unported("Entropic Brew requires target potion slots");
                    break;
                };
                if !slots.iter().take(capacity).any(Option::is_none) { break; }
                let Some(id) = random_potion(c, player, false) else { break };
                if !procure_potion(c, player, id) || c.fault.is_some() { break; }
            }
        }
        PotionId::Fairyinabottle => {
            let amount = (c.cr(player).max_hp * 30 / 100).max(1);
            if c.cr(player).is_dead() {
                let max_hp = c.cr(player).max_hp;
                c.cr_mut(player).hp = amount.min(max_hp);
            } else {
                c.heal(player, amount);
            }
        }
        PotionId::Fruitjuice => gain_max_hp(c, player, 5),
        PotionId::Gigantificationpotion => {
            c.apply_power_to(
                player,
                PowerId::Gigantification,
                1,
                Some(owner),
            );
        }
        PotionId::Liquidmemories => {
            if !c.players[player].discard.is_empty() {
                let cards = c.players[player].discard.clone();
                c.request_card_choice_for_player(player, &cards, PileKind::Discard, potion_resolver(id, player, 5), false);
            }
        }
        PotionId::Luckytonic => {
            c.apply_power_to(player, PowerId::Buffer, 1, Some(owner));
        }
        PotionId::Mazalethsgift => {
            c.apply_power_to(player, PowerId::Ritual, 1, Some(owner));
        }
        PotionId::Orobicacid => {
            // fire: potions/effects.py:668-699
            for card_type in [CardType::Attack, CardType::Skill, CardType::Power] {
                let generated = c.generate_unlocked_owner_cards(player, Some(card_type), 1);
                for g in generated {
                    c.set_temporary_free_this_turn(g);
                    c.add_generated_card(g, PileKind::Hand, crate::combat::PilePos::Bottom, true);
                }
            }
        }
        PotionId::Shacklingpotion => {
            for e in c.hittable_enemies() {
                c.apply_power_to(e, PowerId::ShacklingPotion, 7, Some(owner));
            }
        }
        PotionId::Shipinabottle => {
            let t = player;
            gain_unpowered_block(c, t, 10);
            c.apply_power_to(t, PowerId::BlockNextTurn, 10, Some(owner));
        }
        PotionId::Sneckooil => {
            let t = player;
            c.draw_cards(t, 7, false);
            let hand: Vec<KRef> = c.players[player].hand.iter().copied().collect();
            for k in hand {
                if !c.cards[k as usize].x_cost && c.cards[k as usize].cost >= 0 {
                    let cost = c.run.rngs.combat_energy_costs.next_int(0, 3) as i32;
                    c.cards[k as usize].set_energy_cost(cost, crate::card::CostExpiry::Turn, false);
                }
            }
        }
        PotionId::Soldiersstew => {
            for k in c.pile_cards(player) {
                if c.cards[k as usize].has_tag(CardTag::Strike) {
                    c.cards[k as usize].replay += 1;
                }
            }
        }

        PotionId::Ambergris => {
            let t = player;
            c.heal(t, c.cr(t).max_hp * 50 / 100);
            if !c.is_over {
                c.apply_power_to(t, PowerId::Ambergris, 1, Some(owner));
            }
        }
        PotionId::Foulpotion => {
            for who in c.all_creatures() {
                // FoulPotion.OnUse explicitly excludes pets.
                if c.cr(who).is_alive() && !c.cr(who).is_pet {
                    deal_unpowered_damage(c, owner, who, 12);
                }
            }
        }
        PotionId::Glowwaterpotion => {
            let hand: Vec<KRef> = c.players[player].hand.iter().copied().collect();
            for k in hand {
                exhaust_from_hand(c, k);
            }
            c.draw_cards(player, 10, false);
        }
        PotionId::Potionshapedrock => {
            if let Some(t) = target {
                deal_unpowered_damage(c, owner, t, 15);
            }
        }

        _ => return false,
    }
    true
}

pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    let ChoiceSrc::Potion(id) = r.src else {
        return false;
    };
    match (id, r.step) {
        (
            PotionId::Attackpotion
            | PotionId::Powerpotion
            | PotionId::Skillpotion
            | PotionId::Colorlesspotion,
            0,
        ) => {
            if let Some(&k) = selected.first() {
                if !c.is_over_or_ending() {
                    // PotionModel.OnUse: AddGeneratedCardToCombat records the
                    // potion owner as creator, independently of the recipient.
                    c.add_generated_card_by(k, PileKind::Hand,
                        crate::combat::PilePos::Bottom, r.target);
                }
            }
        }
        (PotionId::Ashwater, 1) => {
            for &k in selected {
                exhaust_from_hand(c, k);
            }
        }
        (PotionId::Gamblersbrew, 2) => {
            // C# CardCmd.DiscardAndDraw: per-card history/hooks, draw, then Sly auto-plays.
            c.discard_cards(r.target.unwrap_or(r.k as CId), selected, selected.len() as i32);
        }
        (PotionId::Touchofinsanity, 3) => {
            if let Some(&k) = selected.first() {
                c.cards[k as usize].set_energy_cost(0, crate::card::CostExpiry::Combat, false);
                c.cards[k as usize].cv.combat_star_cost_override = Some(0);
            }
        }
        (PotionId::Dropletofprecognition, 4) => {
            if let Some(&k) = selected.first() {
                move_to_hand(c, k);
            }
        }
        (PotionId::Liquidmemories, 5) => {
            if let Some(&k) = selected.first() {
                c.cards[k as usize].set_energy_cost(0, crate::card::CostExpiry::Turn, false);
                move_to_hand(c, k);
            }
        }
        _ => return false,
    }
    true
}
