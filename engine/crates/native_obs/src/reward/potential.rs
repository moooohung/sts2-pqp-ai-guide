//! Exact r7 potential contract. Integration must replace the old high-water
//! reward, never add this shaping on top of it.
use serde::{Deserialize,Serialize};
use sts2_sim::run::{RunManager,RunPhase,mp::{MpRun,MpPhase}};
pub const TRAINING_OBJECTIVE:&str="act3_boss_clear_a10_r7_potential_v1";

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
pub enum Contract {
    #[serde(rename="r7-potential-v1")]
    R7PotentialV1,
    /// r7 main launch `reward_objective="terminal_depth"`: a death keeps
    /// DEPTH_CREDIT * floor progress; failures (limits, no progress) do not.
    #[serde(rename="r7-terminal-depth-v1")]
    R7TerminalDepthV1,
    /// terminal_depth plus MENU_DECISION_COST for every decision taken outside
    /// combat. 2026-11-01 the policy collapsed into opening and skipping a card
    /// reward forever: the -1 game-limit failure sits ~10,000 decisions ahead,
    /// beyond what bootstrapped values carry, so a stall looked free next to a
    /// risky fight. The cost makes every repeat lose immediately; nothing is cut.
    #[serde(rename="r7-terminal-depth-menu-cost-v1")]
    R7TerminalDepthMenuCostV1,
}
/// About 9 out-of-combat decisions happen per floor in normal play, so the cost
/// is ~0.0045 per floor against DEPTH_CREDIT/DEPTH_SPAN = 0.029 per floor gained.
pub const MENU_DECISION_COST:f64=0.0005;
/// r7 progress_objective: p = clip((highest_floor - reset_floor) / 51, 0, 1).
pub const DEPTH_SPAN:f64=51.;
pub const DEPTH_CREDIT:f64=1.5;
/// Every native run enters Neow at total_floor 1 (r7 `_prev_floor = 1`).
pub const START_FLOOR:i32=1;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Outcome {Won,Death,Failure}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reward {pub contract:Contract,pub shaping_gamma:f64}
impl Default for Reward {
    fn default()->Self {Self{contract:Contract::R7PotentialV1,shaping_gamma:1.}}
}
/// Public sufficient statistics only: own HP, shared floor and current enemies.
#[derive(Clone,Copy,Debug)]
pub struct State {
    pub seats:usize,
    pub floor:i32,
    pub hp:[i32;4],
    pub max_hp:[i32;4],
    pub enemy_total:i64,
    pub enemy_removed:i64,
    pub run_over:bool,
    /// The run is in a combat phase.
    pub combat:bool,
}
impl State {
    pub fn empty(seats:usize)->Self {
        assert!((1..=4).contains(&seats));
        Self{seats,floor:0,hp:[0;4],max_hp:[0;4],enemy_total:0,enemy_removed:0,run_over:false,combat:false}
    }
    fn enemies(&mut self,combat:&sts2_sim::combat::Combat) {
        for &id in &combat.enemies {
            let enemy=combat.cr(id);
            if enemy.max_hp<=0 {continue;}
            let max=i64::from(enemy.max_hp);
            let hp=if enemy.is_alive(){i64::from(enemy.hp.max(0))}else{0};
            self.enemy_total+=max;
            self.enemy_removed+=max-hp.min(max);
        }
    }
    pub fn solo(run:&RunManager)->Self {
        let mut state=Self::empty(1);
        let context=run.run_ctx();
        state.floor=context.total_floor;
        state.hp[0]=context.hp;state.max_hp[0]=context.max_hp;
        state.run_over=run.phase()==RunPhase::RunOver;
        state.combat=run.phase()==RunPhase::Combat;
        if run.phase()==RunPhase::Combat {
            if let Some(combat)=run.combat() {
                state.hp[0]=combat.player().hp;state.max_hp[0]=combat.player().max_hp;
                state.enemies(combat);
            }
        }
        state
    }
    pub fn party(run:&MpRun)->Self {
        let mut state=Self::empty(run.roster.players().len());
        state.floor=run.world.run.total_floor;
        state.run_over=run.phase==MpPhase::RunOver;
        state.combat=run.phase==MpPhase::Combat;
        for (seat,player) in run.roster.players().iter().enumerate() {
            state.hp[seat]=player.hp;state.max_hp[seat]=player.max_hp;
        }
        if run.phase==MpPhase::Combat {
            if let Some(combat)=run.combat() {
                for seat in 0..state.seats {
                    state.hp[seat]=combat.cr(seat as u8).hp;
                    state.max_hp[seat]=combat.cr(seat as u8).max_hp;
                }
                state.enemies(combat);
            }
        }
        state
    }
    pub fn phi(&self,seat:usize)->f64 {
        assert!(seat<self.seats);
        if self.run_over {return 0.;}
        let mut phi=0.5*(f64::from(self.floor.max(0))/51.).min(1.);
        if self.max_hp[seat]>0 {
            phi+=0.2*f64::from(self.hp[seat].clamp(0,self.max_hp[seat]))/f64::from(self.max_hp[seat]);
        }
        if self.enemy_total>0 {phi+=0.15*self.enemy_removed as f64/self.enemy_total as f64;}
        phi
    }
}
#[derive(Clone,Copy,Debug)]
pub struct Transition {pub seats:usize,pub seat_rewards:[f32;4],pub team_reward:f32}
impl Reward {
    pub fn validate(&self)->Result<(),String> {
        if self.shaping_gamma!=1. {return Err("r7 frozen reward requires shaping_gamma=1".into());}
        Ok(())
    }
    /// r_i = (terminal_team_reward + gamma*Phi_i(next)-Phi_i(now))/N.
    /// Thus sum_i r_i retains a team terminal of +/-1 and uses mean seat Phi;
    /// party size cannot multiply shared floor/combat shaping. Solo is exact r7.
    pub fn transition(&self,before:&State,after:&State,terminal:Option<bool>)->Transition {
        self.transition_outcome(before,after,terminal.map(|won|if won {Outcome::Won}else{Outcome::Death}))
    }
    pub fn transition_outcome(&self,before:&State,after:&State,terminal:Option<Outcome>)->Transition {
        assert_eq!(before.seats,after.seats);
        let mut out=Transition{seats:before.seats,seat_rewards:[0.;4],team_reward:0.};
        let objective=match (terminal,self.contract) {
            (None,_)=>0.,
            (Some(Outcome::Won),_)=>1.,
            (Some(Outcome::Failure),_)|(Some(Outcome::Death),Contract::R7PotentialV1)=>-1.,
            // Total floor never decreases within a run, so `after` is the high-water floor.
            (Some(Outcome::Death),Contract::R7TerminalDepthV1|Contract::R7TerminalDepthMenuCostV1)=>
                -1.+DEPTH_CREDIT*(f64::from((after.floor-START_FLOOR).max(0))/DEPTH_SPAN).min(1.),
        };
        let cost=if self.contract==Contract::R7TerminalDepthMenuCostV1 && !before.combat {MENU_DECISION_COST}else{0.};
        for seat in 0..before.seats {
            let next=if terminal.is_some(){0.}else{after.phi(seat)};
            out.seat_rewards[seat]=((objective-cost+self.shaping_gamma*next-before.phi(seat))/before.seats as f64) as f32;
            out.team_reward+=out.seat_rewards[seat];
        }
        out
    }
}

pub fn solo_objective_complete(run:&RunManager)->bool {
    run.run_ctx().ascension==10 && (run.observation_full_run_victory() ||
        (run.phase()==RunPhase::Event && run.current_act_index()>=2 &&
         run.current_event_id()==Some("THE_ARCHITECT") && run.run_ctx().hp>0 &&
         run.native_choice().is_none()))
}
pub fn party_objective_complete(run:&MpRun)->bool {
    run.world.run.ascension==10 && matches!(run.phase,MpPhase::Event|MpPhase::RunOver) &&
        run.world.run.act_index>=2 && run.world.current_room==Some("THE_ARCHITECT") &&
        run.roster.players().iter().any(|p|p.hp>0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Enemy {hp:i32,max_hp:i32,alive:bool}
    #[derive(Deserialize)]
    struct Snapshot {floor:i32,hp:i32,max_hp:i32,run_over:bool,combat:bool,enemies:Vec<Enemy>}
    impl Snapshot {
        fn state(&self)->State {
            let mut state=State::empty(1);
            state.floor=self.floor;state.hp[0]=self.hp;state.max_hp[0]=self.max_hp;
            state.run_over=self.run_over;
            if self.combat {
                for enemy in &self.enemies {
                    if enemy.max_hp<=0 {continue;}
                    let max=i64::from(enemy.max_hp);
                    state.enemy_total+=max;
                    state.enemy_removed+=max-if enemy.alive{i64::from(enemy.hp.max(0)).min(max)}else{0};
                }
            }
            state
        }
    }
    #[derive(Deserialize)]
    struct Row {before:Snapshot,after:Snapshot,phi_before:f64,phi_after:f64,reward:f64,
        terminated:bool,truncated:bool,objective_succeeded:Option<bool>}
    #[derive(Deserialize)]
    struct Episode {start:usize,end:usize,initial_phi:f64}
    #[derive(Deserialize)]
    struct Fixture {transitions:Vec<Row>,episodes:Vec<Episode>}
    #[test]
    fn seat_local_hp_credit_retains_team_terminal_and_telescopes() {
        let reward=Reward::default();
        let mut initial=State::empty(2);
        initial.floor=1;initial.hp=[80,40,0,0];initial.max_hp=[80,80,0,0];
        let mut next=initial;
        next.floor=2;next.hp[0]=60;next.hp[1]=50;
        next.enemy_total=100;next.enemy_removed=40;
        let shaped=reward.transition(&initial,&next,None);
        assert!(shaped.seat_rewards[1]>shaped.seat_rewards[0]);
        let ended=reward.transition(&next,&next,Some(true));
        assert!((shaped.team_reward+ended.team_reward-(1.-(initial.phi(0)+initial.phi(1))/2.) as f32).abs()<1e-6);
        for seat in 0..2 {
            assert!((shaped.seat_rewards[seat]+ended.seat_rewards[seat]-
                ((1.-initial.phi(seat))/2.) as f32).abs()<1e-6);
        }
    }
    #[test]
    fn terminal_depth_credits_deaths_by_floor_but_never_failures() {
        let reward=Reward{contract:Contract::R7TerminalDepthV1,shaping_gamma:1.};
        let mut start=State::empty(1);start.floor=START_FLOOR;start.hp[0]=80;start.max_hp[0]=80;
        let mut deep=start;deep.floor=18;deep.hp[0]=10;
        let shaped=reward.transition(&start,&deep,None).team_reward as f64;
        let death=reward.transition_outcome(&deep,&deep,Some(Outcome::Death)).team_reward as f64;
        let failure=reward.transition_outcome(&deep,&deep,Some(Outcome::Failure)).team_reward as f64;
        // r7 terminal_depth: death = -1 + 1.5 * (18 - 1) / 51 = -0.5; other = -1.
        assert!((shaped+death-(-0.5-start.phi(0))).abs()<1e-6);
        assert!((shaped+failure-(-1.-start.phi(0))).abs()<1e-6);
        // A stalled game must never beat dying deeper, nor dying at the same floor.
        assert!(failure<death);
        let mut top=deep;top.floor=60;
        assert!((reward.transition_outcome(&top,&top,Some(Outcome::Death)).team_reward as f64+top.phi(0)-0.5).abs()<1e-6);
        let old=Reward::default();
        assert_eq!(old.transition_outcome(&deep,&deep,Some(Outcome::Death)).team_reward,
            old.transition_outcome(&deep,&deep,Some(Outcome::Failure)).team_reward);
    }
    #[test]
    fn menu_cost_makes_reward_screen_stall_lose_to_dying_and_keeps_floors_worth_it() {
        let reward=Reward{contract:Contract::R7TerminalDepthMenuCostV1,shaping_gamma:1.};
        let mut screen=State::empty(1);screen.floor=12;screen.hp[0]=40;screen.max_hp[0]=80;
        // Open/skip on a reward screen: no state change, each repeat now loses.
        let repeat=reward.transition(&screen,&screen,None).team_reward as f64;
        assert!((repeat+MENU_DECISION_COST).abs()<1e-9);
        let mut fight=screen;fight.combat=true;
        assert_eq!(reward.transition(&fight,&fight,None).team_reward,0.);
        // Stalling to the 10,000-decision limit is worse than dying here at once,
        // and the menu cost of a normal floor stays well below one floor of depth credit.
        let stall=10_000.*repeat+reward.transition_outcome(&screen,&screen,Some(Outcome::Failure)).team_reward as f64;
        let die=reward.transition_outcome(&screen,&screen,Some(Outcome::Death)).team_reward as f64;
        assert!(stall<die-1.);
        assert!(9.*MENU_DECISION_COST<0.2*DEPTH_CREDIT/DEPTH_SPAN);
        let old=Reward{contract:Contract::R7TerminalDepthV1,shaping_gamma:1.};
        assert_eq!(old.transition(&screen,&screen,None).team_reward,0.);
        let mut party=State::empty(2);party.floor=12;party.hp=[40,40,0,0];party.max_hp=[80,80,0,0];
        assert!((reward.transition(&party,&party,None).team_reward as f64+MENU_DECISION_COST).abs()<1e-9);
    }
}
