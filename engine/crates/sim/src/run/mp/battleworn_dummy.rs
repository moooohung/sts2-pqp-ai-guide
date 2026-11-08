use super::{MpRun, MpPhase, PlayerRewards};
use crate::{run::{events::{EventOptionIdentity, EventState}, manager::battleworn_dummy::{setting, ENCOUNTERS}}, tables::RoomType};

impl MpRun {
    pub(super) fn start_battleworn_dummy(&mut self, seat: usize, event: EventState, option: EventOptionIdentity)
        -> Result<(), &'static str> {
        let tier = setting(option)?;
        self.events[seat] = Some(event);
        let combat = self.roster.enter_combat_id(self.world.run.clone(),
            self.world.run.rngs.seed, ENCOUNTERS[tier], RoomType::Monster)?;
        self.combat = Some(combat);
        self.world.current_room = Some(ENCOUNTERS[tier].name());
        self.world.run.room_type = RoomType::Monster;
        self.phase = MpPhase::Combat;
        Ok(())
    }

    pub(super) fn dummy_rewards(&mut self, seat: usize, tier: usize, timed_out: bool)
        -> Result<PlayerRewards, &'static str> {
        let mut rewards = PlayerRewards::default();
        if !timed_out && self.roster.player(seat).unwrap().hp > 0 {
            match tier {
                0 => {
                    let character = self.roster.player(seat).unwrap().character;
                    let pool = super::super::potion_pools::cards(character).ok_or("Dummy potion pool")?;
                    let pick = self.roster.player_mut(seat).unwrap().rngs.rewards.choice_index(pool.len());
                    rewards.extra_potions.push(Some(pool[pick]));
                }
                1 => {
                    let mut event = self.events[seat].take().ok_or("Dummy owner event missing")?;
                    self.upgrade_random_event_cards(seat, &mut event, 2);
                }
                2 => rewards.extra_relics.push(Some(self.pull_event_relic_from_front(seat)?)),
                _ => return Err("invalid Dummy setting"),
            }
        }
        self.events[seat] = None;
        self.event_done[seat] = true;
        Ok(rewards)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{creature::Side, run::mp::{Character, PlayerSpec}};
    #[test]
    fn battleworn_dummy_shared_combat_rewards_each_owner_and_timeout_grants_nothing() {
        for tier in 0..3 {
            for timeout in [false,true] {
                let specs=[PlayerSpec{net_id:7,character:Character::Ironclad},
                    PlayerSpec{net_id:42,character:Character::Silent}];
                let mut run=MpRun::new("73001",&specs,0).unwrap();
                run.world.current_room=Some("BATTLEWORN_DUMMY");
                run.phase=MpPhase::Event;
                run.begin_generic_event(crate::identity::EventId::BattlewornDummy).unwrap();
                let counter=run.shared_event_selection_rng.counter;
                run.choose_event_option(42,1,tier).unwrap();
                assert_eq!(run.phase,MpPhase::Event);
                assert!(run.combat.is_none());
                assert_eq!(run.shared_event_selection_rng.counter,counter);
                run.choose_event_option(7,0,tier).unwrap();
                assert_eq!(run.shared_event_selection_rng.counter,counter+1);
                let c=run.combat.as_mut().unwrap();
                if timeout {
                    for _ in 0..3 { c.fire_after_turn_end(Side::Enemy); }
                } else {
                    let e=c.enemies[0];c.force_kill_creature(e);c.check_combat_end();
                }
                run.phase=MpPhase::Rewards;
                run.offer_rewards().unwrap();
                for seat in 0..2 {
                    let r=&run.rewards.as_ref().unwrap()[seat];
                    assert!(r.gold.is_empty() && r.cards.is_empty() && r.potion.is_none());
                    assert_eq!(r.extra_potions.len(),usize::from(tier==0&&!timeout));
                    assert_eq!(r.extra_relics.len(),usize::from(tier==2&&!timeout));
                    assert_eq!(run.roster.player(seat).unwrap().deck.iter().filter(|c|c.upgraded).count(),
                        if tier==1&&!timeout {2}else{0});
                }
            }
        }
    }
}
