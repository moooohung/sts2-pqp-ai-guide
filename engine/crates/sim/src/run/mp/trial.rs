use crate::character::CardPool;
use super::*;
use crate::run::events::trial::{self, Outcome};

impl MpRun {
    pub(super) fn choose_trial(&mut self, seat: usize, mut event: EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let outcome = trial::choose(&mut event, option)?;
        match outcome {
            Outcome::Continue => { self.events[seat] = Some(event); return Ok(()); }
            Outcome::Abandon => {
                for seat in 0..self.roster.players().len() {
                    self.roster.player_mut(seat).unwrap().hp = 0;
                }
                self.world.run.hp = 0;
                self.phase = MpPhase::RunOver;
                return Ok(());
            }
            Outcome::MerchantGuilty => {
                self.add_event_card(seat, CardId::Regret)?;
                for _ in 0..2 {
                    let relic = self.pull_event_relic_from_front(seat)?;
                    self.attach_event_relic(seat, relic)?;
                    self.record_relic_seen(seat, relic);
                }
            }
            Outcome::MerchantInnocent | Outcome::NondescriptInnocent => {
                let upgrade = matches!(outcome, Outcome::MerchantInnocent);
                self.add_event_card(seat, if upgrade { CardId::Shame } else { CardId::Doubt })?;
                let cards = if upgrade { self.upgradable_event_cards(seat) } else { self.transformable_event_cards(seat) };
                let count = cards.len().min(2);
                if count > 0 {
                    let all = cards.len() <= 2;
                    self.events[seat] = Some(event);
                    self.begin_event_card_modal(seat, cards, count, count, false,
                        if upgrade { EventCardAction::Upgrade } else { EventCardAction::Transform })?;
                    if all {
                        let net = self.roster.player(seat).unwrap().net_id;
                        self.select_event_cards(net, seat, &(0..count).collect::<Vec<_>>())?;
                    }
                    return Ok(());
                }
            }
            Outcome::NobleGuilty => self.apply_event_effect(seat, EventEffect::Heal(10), &mut event)?,
            Outcome::NobleInnocent => {
                self.add_event_card(seat, CardId::Regret)?;
                self.apply_event_effect(seat, EventEffect::GainGold(300), &mut event)?;
            }
            Outcome::NondescriptGuilty => {
                self.add_event_card(seat, CardId::Doubt)?;
                let character = self.roster.player(seat).unwrap().character;
                let mut pool = super::super::super::card_pools::cards(character.into(), true)?;
                if self.roster.player(seat).unwrap().relics.iter().any(|r| r.enabled && r.id == RelicId::DingyRug) {
                    for id in super::super::super::card_pools::cards(CardPool::Colorless, true)? {
                        if !pool.contains(&id) { pool.push(id); }
                    }
                }
                let mut rewards = Vec::new();
                for _ in 0..2 {
                    let cards = self.make_other_regular_reward_cards_with_upgrade_roll(seat, &pool, 3, true, false)?;
                    rewards.push(super::super::PlayerRewards { cards, ..Default::default() });
                }
                self.events[seat] = Some(event);
                self.event_card_reward_queues[seat] = rewards;
                return Ok(());
            }
        }
        self.finish_event_seat(seat);
        Ok(())
    }
}
