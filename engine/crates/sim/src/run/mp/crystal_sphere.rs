use crate::character::CardPool;
use super::MpRun;
use crate::{card::Card, identity::EventOptionId, run::events::{EventEffect, EventOption, EventOptionIdentity, EventParameter, EventState, SphereItemKind, SphereState},
    tables::{CardId, CardRarity, PotionId, RelicId, POTION_INFO}};

#[derive(Clone, Debug)]
pub(super) enum SphereLoot {
    Potion(PotionId),
    Relic(RelicId),
    Cards { cards: Vec<Card>, rarity: CardRarity, reroll: bool },
}

use crate::run::events::sphere_reward_option_id as option_id;

impl MpRun {
    fn sphere_relic_obtained(&mut self, seat: usize, index: usize) -> Result<(), &'static str> {
        let mut next_id = self.roster.next_card_id;
        let player = self.roster.player_mut(seat).unwrap();
        for loot in &mut self.sphere_loot[seat] {
            let Some(SphereLoot::Cards { cards, .. }) = loot else { continue; };
            crate::run::reward_modifiers::apply(&mut player.relics[index..=index],
                cards, seat as u8, Some(crate::tables::RoomType::Event), true,
                &mut next_id, &mut self.world.run.rngs.niche, |card, next_id| {
                    let enchantments = card.enchantments.clone();
                    *next_id += 1;
                    let mut upgraded = Card::from_factory(card.id, true, *next_id)
                        .ok_or("sphere reward upgrade factory")?;
                    upgraded.owner = card.owner;
                    for (name, amount) in enchantments {
                        if !crate::card::enchant::apply_static_enchantment(&mut upgraded, name, amount) {
                            return Err("sphere reward upgrade enchantment");
                        }
                    }
                    *card = upgraded;
                    Ok(())
                })?;
        }
        self.roster.next_card_id = next_id;
        Ok(())
    }

    fn sphere_cards(&mut self, seat: usize, event: &mut EventState, rarity: CardRarity)
        -> Result<Vec<Card>, &'static str> {
        let player = self.roster.player(seat).unwrap();
        let mut pool = super::super::card_pools::cards(player.character.into(), true)?;
        if player.relics.iter().any(|r| r.id == RelicId::DingyRug && r.enabled) {
            for id in super::super::card_pools::cards(CardPool::Colorless, true)? {
                if !pool.contains(&id) { pool.push(id); }
            }
        }
        let mut cards: Vec<Card> = Vec::new();
        for _ in 0..3 {
            let candidates: Vec<_> = pool.iter().copied()
                .filter(|id| !cards.iter().any(|c| c.id == *id))
                .filter(|id| crate::tables::card_info(*id, false).is_some_and(|c| c.rarity == rarity))
                .collect();
            if candidates.is_empty() { return Err("crystal sphere card reward pool exhausted"); }
            let id = candidates[event.rng.choice_index(candidates.len())];
            let roll = event.rng.next_float(1.0);
            let scaling = if self.world.run.ascension >= 7 { 0.125 } else { 0.25 };
            let odds = if rarity == CardRarity::Rare { 0.0 } else {
                self.world.run.act_index as f64 * scaling
            };
            self.roster.next_card_id += 1;
            let mut card = Card::from_factory(id, roll <= odds, self.roster.next_card_id)
                .ok_or("crystal sphere card factory")?;
            card.owner = seat as u8;
            cards.push(card);
        }
        let mut next_id = self.roster.next_card_id;
        super::rewards::apply_card_reward_modifiers(self.roster.player_mut(seat).unwrap(),
            &mut cards, &mut next_id, Some(crate::tables::RoomType::Event), &mut self.world.run.rngs.niche)?;
        self.roster.next_card_id = next_id;
        Ok(cards)
    }

    fn sphere_reward_options(&mut self, seat: usize, event: &mut EventState) {
        event.options.clear();
        for (i, loot) in self.sphere_loot[seat].iter().enumerate() {
            let Some(loot) = loot else { continue; };
            event.options.push_sphere_reward(i,0,"Skip reward");
            match loot {
                SphereLoot::Cards { cards, reroll, .. } => {
                    for c in 0..cards.len() {
                        event.options.push_sphere_reward(i,c+1,"Take card");
                    }
                    if *reroll {
                        event.options.push_sphere_reward(i,5,"Reroll");
                    }
                    if self.roster.player(seat).unwrap().relics.iter()
                        .any(|r| r.id == RelicId::PaelsWing && r.enabled) {
                        event.options.push_sphere_reward(i,6,"Sacrifice");
                    }
                }
                _ => event.options.push_sphere_reward(i,1,"Take reward"),
            }
        }
    }

    pub(super) fn choose_crystal_sphere(&mut self, seat: usize, mut event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        if let EventParameter::SphereReward { offer, action } = option.parameter {
            let i = usize::from(offer);
            let action = usize::from(action);
            let loot = self.sphere_loot[seat].get_mut(i).and_then(Option::take)
                .ok_or("sphere reward missing")?;
            match (action, loot) {
                (0, _) => {}
                (1, SphereLoot::Potion(id)) => {
                    let player = self.roster.player_mut(seat).unwrap();
                    if player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                        // Denied procurement still completes PotionReward.OnSelect.
                    } else if let Some(slot) = player.potions.iter_mut().find(|p| p.is_none()) {
                        *slot = Some(id);
                        self.mark_local_potion_seen(seat, id);
                    } else {
                        self.sphere_loot[seat][i] = Some(SphereLoot::Potion(id));
                    }
                }
                (1, SphereLoot::Relic(id)) => {
                    let index = self.attach_event_relic(seat, id)?;
                    self.sphere_relic_obtained(seat, index)?;
                    self.record_relic_seen(seat, id);
                }
                (5, SphereLoot::Cards { rarity, reroll: true, .. }) => {
                    let cards = self.sphere_cards(seat, &mut event, rarity)?;
                    self.sphere_loot[seat][i] = Some(SphereLoot::Cards { cards, rarity, reroll: false });
                }
                (6, SphereLoot::Cards { .. }) => {
                    let wing = self.roster.player_mut(seat).unwrap().relics.iter_mut()
                        .find(|r| r.id == RelicId::PaelsWing && r.enabled).ok_or("sphere sacrifice missing")?;
                    wing.counter += 1;
                    if wing.counter % 2 == 0 {
                        let id = self.pull_event_relic_from_front(seat)?;
                        let index = self.attach_event_relic(seat, id)?;
                        self.sphere_relic_obtained(seat, index)?;
                        self.record_relic_seen(seat, id);
                    }
                }
                (a @ 1..=4, SphereLoot::Cards { cards, .. }) => {
                    let card = cards.get(a - 1).ok_or("invalid sphere reward card")?.clone();
                    super::cards::add(&mut self.roster, &mut self.world.run, seat, card)?;
                }
                _ => return Err("invalid sphere reward action"),
            }
            self.sphere_reward_options(seat, &mut event);
        } else {
            if event.crystal.is_none() {
                let picks = match option.id {
                    EventOptionId::CrystalSphereInitialUncoverFuture => {
                        if self.roster.player(seat).unwrap().gold < event.ints[0] {
                            return Err("crystal sphere insufficient gold");
                        }
                        self.apply_event_effect(seat, EventEffect::LoseGold(event.ints[0]), &mut event)?;
                        3
                    }
                    EventOptionId::CrystalSphereInitialPaymentPlan => { self.add_event_card(seat, CardId::Debt)?; 6 }
                    _ => return Err("invalid crystal sphere option"),
                };
                event.crystal = Some(SphereState::new(&mut event.rng, picks));
            } else {
                let curses = event.crystal.as_mut().unwrap().click(option)?;
                for _ in 0..curses { self.add_event_card(seat, CardId::Doubt)?; }
            }
            let sphere = event.crystal.as_ref().unwrap();
            if sphere.picks_remaining > 0 {
                event.options.replace_sphere(sphere.options());
                self.events[seat] = Some(event);
                return Ok(());
            }
            let revealed = sphere.revealed_rewards();
            let pool = super::super::potion_pools::cards(self.roster.player(seat).unwrap().character)
                .ok_or("sphere potion pool missing")?;
            // ToReward draws every potion before any Reward.Populate calls.
            let mut potions = Vec::new();
            for item in &revealed {
                if let SphereItemKind::Potion(rarity) = item {
                    let candidates: Vec<_> = pool.iter().copied()
                        .filter(|id| POTION_INFO[id.index()].rarity == *rarity).collect();
                    if candidates.is_empty() { return Err("sphere potion pool exhausted"); }
                    potions.push(candidates[event.rng.choice_index(candidates.len())]);
                }
            }
            let mut potions = potions.into_iter();
            let mut loot = Vec::new();
            for item in revealed {
                let reward = match item {
                    SphereItemKind::Gold(amount) => {
                        let amount = event.rng.next_int(amount as i64, amount as i64) as i32;
                        self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run, amount);
                        continue;
                    }
                    SphereItemKind::Potion(_) => SphereLoot::Potion(potions.next().ok_or("sphere potion missing")?),
                    SphereItemKind::Relic => {
                        let roll = event.rng.next_float(1.0);
                        let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
                        SphereLoot::Relic(self.world.pull_player_relic_from_front(&self.roster, seat, rarity))
                    }
                    SphereItemKind::Card(rarity) => {
                        let cards = self.sphere_cards(seat, &mut event, rarity)?;
                        let reroll = self.roster.player(seat).unwrap().relics.iter()
                            .any(|r| r.id == RelicId::Driftwood && r.enabled);
                        SphereLoot::Cards { cards, rarity, reroll }
                    }
                    SphereItemKind::Curse => continue,
                };
                loot.push(Some(reward));
            }
            self.sphere_loot[seat] = loot;
            self.sphere_reward_options(seat, &mut event);
        }
        if event.options.is_empty() {
            self.sphere_loot[seat].clear();
            self.finish_event_seat(seat);
        } else {
            self.events[seat] = Some(event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Character, MpPhase, PlayerSpec};
    #[test]
    fn obtained_egg_updates_only_owners_pending_cards_without_replaying_other_relics() {
        let mut run = MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap();
        run.phase = MpPhase::Event;
        run.begin_generic_event(crate::identity::EventId::CrystalSphere).unwrap();
        for seat in 0..2 {
            let mut card = Card::from_factory(CardId::StrikeIronclad, false, 100 + seat as u32).unwrap();
            card.owner = seat as u8;
            crate::card::enchant::apply_static_enchantment(&mut card, "Sharp", 1);
            run.sphere_loot[seat] = vec![
                Some(SphereLoot::Cards { cards: vec![card], rarity: CardRarity::Common, reroll: false }),
                Some(SphereLoot::Relic(RelicId::MoltenEgg)),
            ];
            let mut event = run.events[seat].take().unwrap();
            run.sphere_reward_options(seat, &mut event);
            run.events[seat] = Some(event);
        }
        let mut wing = crate::relic::Relic::new(RelicId::WingCharm);
        wing.owner = 1;
        run.roster.player_mut(1).unwrap().relics.push(wing);
        let rewards = run.roster.player(1).unwrap().rngs.rewards.counter;
        let niche = run.world.run.rngs.niche.counter;
        let options = run.event_options(42, 1).unwrap();
        let take = options.iter().position(|id| *id == option_id(1, 1)).unwrap();
        run.choose_event_option(42, 1, take).unwrap();
        let Some(SphereLoot::Cards { cards, .. }) = &run.sphere_loot[1][0] else { panic!() };
        assert!(cards[0].upgraded);
        assert_eq!(cards[0].owner, 1);
        assert!(cards[0].enchantments.iter().any(|(name, amount)| *name == crate::identity::EnchantmentId::Sharp && *amount == 1));
        let Some(SphereLoot::Cards { cards, .. }) = &run.sphere_loot[0][0] else { panic!() };
        assert!(!cards[0].upgraded);
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, rewards);
        assert_eq!(run.world.run.rngs.niche.counter, niche);
    }
    #[test]
    fn crystal_cost_picks_and_debt_are_owner_local_until_all_loot_resolves() {
        let mut run = MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap();
        run.phase = MpPhase::Event;
        run.begin_generic_event(crate::identity::EventId::CrystalSphere).unwrap();
        let cost = run.events[0].as_ref().unwrap().ints[0];
        let rng = run.roster.player(1).unwrap().rngs.rewards.counter;
        run.choose_event_option(7, 0, 0).unwrap();
        assert_eq!(run.roster.player(0).unwrap().gold, 99 - cost);
        assert_eq!(run.roster.player(1).unwrap().gold, 99);
        assert_eq!(run.events[0].as_ref().unwrap().crystal.as_ref().unwrap().picks_remaining, 3);
        run.choose_event_option(42, 1, 1).unwrap();
        assert_eq!(run.roster.player(1).unwrap().deck.last().unwrap().id, CardId::Debt);
        assert_eq!(run.events[1].as_ref().unwrap().crystal.as_ref().unwrap().picks_remaining, 6);
        for (seat, net, picks) in [(0, 7, 3), (1, 42, 6)] {
            for _ in 0..picks { run.choose_event_option(net, seat, 0).unwrap(); }
            while !run.event_done[seat] {
                // First option skips one retained, fully populated reward.
                run.choose_event_option(net, seat, 0).unwrap();
            }
        }
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, rng);
    }
}
