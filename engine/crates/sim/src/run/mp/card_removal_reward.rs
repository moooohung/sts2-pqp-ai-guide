//! CardRemovalReward.OnSelect -> RewardSynchronizer.DoUnsyncedCardRemoval.
use super::{MpRun,MpPhase};
impl MpRun {
    pub fn card_removal_reward_cards(&self,net:u64,seat:usize,reward:usize)->Result<Vec<u32>,&'static str>{
        self.roster.actor(net,seat)?;
        if self.phase!=MpPhase::Rewards || self.rewards.as_ref().and_then(|r|r.get(seat))
            .and_then(|r|r.card_removals.get(reward))!=Some(&false) {
            return Err("card removal reward unavailable");
        }
        let deck=&self.roster.player(seat).unwrap().deck;
        Ok(super::super::manager::RunManager::removable_card_indices(deck).into_iter()
            .map(|i|deck[i].instance_id).collect())
    }

    /// Cancelable=true, RequireManualConfirmation=true. Cancelling returns
    /// false from OnSelect and therefore leaves the reward available.
    pub fn take_card_removal_reward(&mut self,net:u64,seat:usize,reward:usize,index:Option<usize>)
        ->Result<(),&'static str>{
        let cards=self.card_removal_reward_cards(net,seat,reward)?;
        let Some(index)=index else{return Ok(());};
        let id=*cards.get(index).ok_or("invalid removal card")?;
        let p=self.roster.player_mut(seat).unwrap();
        let index=p.deck.iter().position(|c|c.instance_id==id).ok_or("removal card missing")?;
        if p.deck[index].id==crate::tables::CardId::SpoilsMap {
            return Err("unported Spoils Map removal callback");
        }
        p.remove_deck_card(index);
        self.rewards.as_mut().unwrap()[seat].card_removals[reward]=true;
        Ok(())
    }
}
