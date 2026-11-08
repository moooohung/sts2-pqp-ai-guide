//! ColorfulPhilosophers.cs: owner-local pool options and three independent rewards.
use super::{MpRun, PlayerRewards};
use crate::{identity::EventOptionId, run::events::{EventOption, EventOptionIdentity, EventState}, tables::CardRarity};

const POOLS: [crate::character::Character; 5] = [crate::character::Character::Necrobinder, crate::character::Character::Ironclad, crate::character::Character::Regent, crate::character::Character::Silent, crate::character::Character::Defect];
const OPTIONS: [&str; 3] = ["pool_1", "pool_2", "pool_3"];

impl MpRun {
    pub(super) fn initialize_philosophers(&self, seat: usize, event: &mut EventState) {
        let owner=self.roster.player(seat).unwrap().character;
        let mut pools:Vec<_>=(0..POOLS.len()).filter(|&i|POOLS[i]!=owner).collect();
        while pools.len()>3 {
            let index=event.rng.choice_index(pools.len());
            pools.remove(index);
        }
        event.options.clear();
        for (i,pool) in pools.into_iter().enumerate() {
            event.ints[i]=pool as i32;
            event.options.push(EventOption::new(OPTIONS[i],POOLS[pool].name(),"Three card rewards"));
        }
    }

    pub(super) fn offer_philosopher_rewards(&mut self,seat:usize,mut event:EventState,option:EventOptionIdentity)
        ->Result<(), &'static str> {
        let index=option.offer_index(EventOptionId::ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant)
            .ok_or("invalid philosopher option")?;
        let pool=POOLS[event.ints[index] as usize];
        let mut rewards=Vec::new();
        for rarity in [CardRarity::Common,CardRarity::Uncommon,CardRarity::Rare] {
            // NoRarityModification | NoCardPoolModifications; late reward
            // modifiers still run, just as CardReward.Populate does in C#.
            let cards=self.uniform_event_cards(seat,pool.into(),rarity,3,true,false,true)?;
            rewards.push(PlayerRewards{cards,..Default::default()});
        }
        event.options.clear();
        self.events[seat]=Some(event);
        self.event_card_reward_queues[seat]=rewards;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, MpPhase, PlayerSpec};

    #[test]
    fn philosophers_offer_three_other_color_rarities_to_each_owner() {
        for character in [Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder] {
            let specs=[PlayerSpec{net_id:100,character:Character::Ironclad},PlayerSpec{net_id:101,character}];
            let mut run=MpRun::new("7100000611",&specs,0).unwrap();
            run.world.current_room=Some("COLORFUL_PHILOSOPHERS");
            run.phase=MpPhase::Event;
            run.begin_generic_event(crate::identity::EventId::ColorfulPhilosophers).unwrap();
            let event=run.events[1].as_ref().unwrap();
            assert_eq!(event.options.len(),3);
            let mut expected_rng=crate::rng::Rng::new(run.world.run.rngs.seed.wrapping_add(1)
                .wrapping_add(crate::rng::deterministic_hash64("COLORFUL_PHILOSOPHERS")));
            let mut expected:Vec<_>=POOLS.into_iter().filter(|&p|p!=character).collect();
            let pick=expected_rng.choice_index(expected.len());
            expected.remove(pick);
            assert_eq!(event.ints[..3].iter().map(|&i|POOLS[i as usize]).collect::<Vec<_>>(),expected);
            let pool=expected[0];
            let other=format!("{:?}",run.roster.player(0).unwrap().rngs);
            let before=run.roster.player(1).unwrap().rngs.rewards.counter;
            run.choose_event_option(101,1,0).unwrap();
            assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter-before,18);
            assert_eq!(run.event_card_reward_queues[1].len(),3);
            for (reward,rarity) in run.event_card_reward_queues[1].iter()
                .zip([CardRarity::Common,CardRarity::Uncommon,CardRarity::Rare]) {
                assert_eq!(reward.cards.len(),3);
                let allowed=crate::run::card_pools::cards(pool.into(),true).unwrap();
                assert!(reward.cards.iter().all(|c|c.owner==1&&c.rarity==rarity&&allowed.contains(&c.id)));
            }
            for _ in 0..3 {run.select_event_cards(101,1,&[]).unwrap();}
            assert!(run.event_done[1]);
            assert!(!run.event_done[0]);
            assert_eq!(format!("{:?}",run.roster.player(0).unwrap().rngs),other);
        }
    }
}
