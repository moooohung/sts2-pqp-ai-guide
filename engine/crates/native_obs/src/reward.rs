//! Native main reward contract, shared with browser trajectories.
pub mod potential;
pub use potential::{Reward,State,Transition,Outcome,Contract,solo_objective_complete,party_objective_complete,TRAINING_OBJECTIVE};
/// Credit each seat's own amount to its latest action, or its boundary ledger
/// when no action exists. Persistent collection supervises the waiting-state
/// critic separately; never redistribute that seat's reward to a teammate.
pub fn credit(rewards:&[f32],latest:impl ExactSizeIterator<Item=Option<usize>>,pending:&mut[f32],mut add:impl FnMut(usize,f32)){
    let seats=latest.len();
    assert!(seats>0 && seats==pending.len() && seats==rewards.len());
    for (seat,index) in latest.enumerate(){
        if let Some(index)=index{add(index,rewards[seat])}else{pending[seat]+=rewards[seat]}
    }
}
#[cfg(test)]
mod credit_tests{
    #[test]
    fn team_rewards_reach_each_seats_latest_action_or_pending_credit(){
        let mut samples=[0f32;2];let mut pending=[0f32;2];
        super::credit(&[1.,1.],[Some(0),None].into_iter(),&mut pending,|i,r|samples[i]+=r);
        assert_eq!(samples,[1.,0.]);assert_eq!(pending,[0.,1.]);
        samples[1]=std::mem::take(&mut pending[1]);
        super::credit(&[2.,2.],[Some(0),Some(1)].into_iter(),&mut pending,|i,r|samples[i]+=r);
        assert_eq!(samples,[3.,3.]);assert_eq!(samples.iter().sum::<f32>(),6.);
    }
}
