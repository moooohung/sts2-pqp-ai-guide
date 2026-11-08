//! CPU/WASM driver of the reviewed solo and multiplayer dispatchers.
use super::*;
use sts2_sim::run::mp::{MpRun,PlayerSpec};

pub(super) enum Run { Solo(RunManager),Party(MpRun) }
pub(super) struct PartyGame {
 pub run:Run,pub encoder:NativeEncoder,pub obs:Box<NativeObsV1>,pub decision:DecisionId,pub seats:usize,
 joint:bool,relational:bool,team_active:bool,team:Option<Box<TeamObsV2>>,
 between:Option<sts2_sim::between::Recorder>,
}
impl PartyGame {
 pub fn start_mode(seed:u64,characters:&[Character],ascension:u8,joint:bool,relational:bool)->Result<Self,String>{
  if !(1..=MAX_SEATS).contains(&characters.len()){return Err("party requires 1-4 seats".into())}
  let mut run=if characters.len()==1{Run::Solo(RunManager::new_for_character(seed,RunOptions{
   ascension:ascension as i32,saved_wongo_points:Some(0),..RunOptions::default()},characters[0]))}
  else{Run::Party(MpRun::new(&seed.to_string(),&characters.iter().enumerate().map(|(i,&character)|PlayerSpec{
   net_id:100+i as u64,character}).collect::<Vec<_>>(),ascension as i32)?)};
  if !joint {if let Run::Party(g)=&mut run {g.enable_legacy_replay_adapter();}}
  let mut game=Self{run,encoder:sts2_policy::native::encoder_for_policy(relational),obs:Box::new(NativeObsV1::new()),joint,relational,team_active:false,team:None,between:None,
   decision:DecisionId{episode:seed,generation:0,actor:0},seats:characters.len()};
  if game.refresh()?!=Status::Continue{return Err("run did not start actionable".into())}
  Ok(game)
 }
 pub fn refresh(&mut self)->Result<Status,String>{
  self.team_active=false;
  if let Run::Party(g)=&mut self.run{
   if self.joint {NativeEncoder::advance_team(g,&mut self.decision)}
    else {NativeEncoder::advance_multiplayer(g,&mut self.decision)}.map_err(|e|format!("{e:?}"))?;
  }
  if match &self.run{Run::Solo(g)=>sts2_native_obs::reward::solo_objective_complete(g),
   Run::Party(g)=>sts2_native_obs::reward::party_objective_complete(g)}{
   return Ok(Status::Terminal{won:true})
  }
  if self.joint {
   if let Run::Party(g)=&self.run {
    if g.phase==sts2_sim::run::mp::MpPhase::Combat {
     self.decision.actor=0;
     let team=self.team.get_or_insert_with(||Box::new(TeamObsV2::new()));
     self.encoder.encode_team(g,self.decision,&mut self.obs,team).map_err(|e|format!("{e:?}"))?;
     self.team_active=true;
     return match team.view().map_err(|e|format!("{e:?}"))?.status {
      DecisionStatus::Actionable=>Ok(Status::Continue),
      DecisionStatus::Terminal=>Ok(Status::Terminal{won:false}),
      DecisionStatus::GameplayFault=>Err("team gameplay fault".into()),
      DecisionStatus::Waiting=>Err("team waiting after automatic queue advancement".into()),
     }
    }
   }
  }
  for offset in 0..self.seats{
   let actor=((self.decision.actor as usize+offset)%self.seats) as u8;
   let decision=DecisionId{actor,..self.decision};
   match &self.run{
    Run::Solo(g)=>self.encoder.encode_solo(g,decision,&mut self.obs),
    Run::Party(g)=>self.encoder.encode_multiplayer(g,decision,100+actor as u64,&mut self.obs),
   }.map_err(|e|format!("native encode: {e:?}"))?;
   match self.obs.view().map_err(|e|format!("{e:?}"))?.status{
    DecisionStatus::Actionable=>{self.decision.actor=actor;return Ok(Status::Continue)}
    DecisionStatus::Terminal=>return Ok(Status::Terminal{won:match &self.run{Run::Solo(g)=>g.observation_full_run_victory(),Run::Party(g)=>sts2_native_obs::reward::party_objective_complete(g)}}),
    DecisionStatus::GameplayFault=>return Err("encoder published gameplay fault".into()),
    DecisionStatus::Waiting=>{},
   }
  }
  Err("all seats waiting after automatic advancement".into())
 }
 pub fn step(&mut self,slot:usize)->Result<Status,String>{
  let team_active=self.team_active;
  match &mut self.run{
   Run::Solo(g)=>self.encoder.dispatch_solo(g,&mut self.decision,&mut self.obs,slot),
   Run::Party(g)=>{let net=100+self.decision.actor as u64;
    if team_active {self.encoder.dispatch_team(g,&mut self.decision,&mut self.obs,self.team.as_mut().unwrap(),slot)}
    else {self.encoder.dispatch_multiplayer(g,&mut self.decision,net,&mut self.obs,slot)}},
  }.map_err(|e|format!("native dispatch: {e:?}"))?;
  if !team_active {self.decision.actor=((self.decision.actor as usize+1)%self.seats) as u8;}
  self.refresh()
 }
 pub fn is_joint(&self)->bool{self.joint}
 /// The current per-seat decision as a search root; team (joint) decisions are not searched.
 pub fn search_root(&self)->Option<super::search::Root>{
  if self.team_active || self.phase()!="COMBAT" {return None;}
  let run=match &self.run{Run::Solo(g)=>super::search::SimRun::Solo(g.clone()),Run::Party(g)=>super::search::SimRun::Party(g.clone())};
  Some(super::search::Root{run,decision:self.decision,seats:self.seats,net_base:100})
 }
 /// The reward-reopen guard (`search::reopen_guard`) for the current per-seat decision.
 pub fn reopen_guard(&self,slots:&[i32])->Option<Vec<bool>>{
  match &self.run{Run::Solo(g) if !self.team_active=>super::search::reopen_guard(g,&self.obs,slots),_=>None}
 }
 pub fn enable_replay_recording(&mut self){if let Run::Party(g)=&mut self.run{g.enable_queue_recording();}}
 pub fn begin_between(&mut self,index:usize)->(sts2_sim::between::Capture,sts2_native_obs::replay::between::Origin){
  let origin=sts2_native_obs::replay::between::origin(&self.game(),index);
  let recorder=self.between.get_or_insert_with(Default::default);
  (recorder.begin(index as u64),origin)
 }
 pub fn between(&self,origin:sts2_native_obs::replay::between::Origin,frame:&Value,terminal:bool)->Result<Value,String>{
  self.projection().between(self.between.as_ref().unwrap(),origin,frame,terminal)
 }
 pub fn observation(&self)->Result<ObservationView<'_>,String>{
  if self.team_active {self.team.as_ref().unwrap().view()}else{self.obs.view()}.map_err(|e|format!("{e:?}"))
 }
 pub fn action_identity(&self,slot:usize)->Result<ActionIdentity,String>{
  if self.team_active {
   let binding=self.team.as_ref().unwrap().binding(slot,self.decision).map_err(|e|format!("{e:?}"))?;
   Ok(ActionIdentity::new(self.decision,binding.actor,slot as i32,binding))
  }else{
   let net=if self.seats==1{1}else{100+self.decision.actor as u64};
   let binding=self.obs.binding(slot,self.decision,net).map_err(|e|format!("{e:?}"))?;
   Ok(ActionIdentity::new(self.decision,self.decision.actor,slot as i32,binding))
  }
 }
 fn game(&self)->sts2_native_obs::replay::Game<'_>{match &self.run{
  Run::Solo(g)=>sts2_native_obs::replay::Game::Solo(g),Run::Party(g)=>sts2_native_obs::replay::Game::Party(g)}}
 pub fn floor(&self)->u32{self.game().floor()}
 pub fn reward_state(&self)->sts2_native_obs::reward::State{match &self.run{
  Run::Solo(g)=>sts2_native_obs::reward::State::solo(g),Run::Party(g)=>sts2_native_obs::reward::State::party(g)}}
 pub fn act(&self)->u64{self.game().act()}
 pub fn combat(&self)->Option<&sts2_sim::combat::Combat>{match &self.run{Run::Solo(g)=>g.combat(),Run::Party(g)=>g.combat()}}
 pub fn phase(&self)->&'static str{match &self.run{
  Run::Solo(g)=>g.phase().name(),Run::Party(g)=>{use sts2_sim::run::mp::MpPhase::*;
   match g.phase{Neow|Event=>"EVENT",Map=>"MAP_CHOICE",Combat=>"COMBAT",Rewards=>"CARD_REWARD",Shop=>"SHOP",
    RestSite=>"REST_SITE",Treasure=>"TREASURE",RunOver=>"RUN_OVER"}}
 }}
 pub fn inventory(&self)->Value{self.game().inventory()}
 fn projection(&self)->sts2_native_obs::replay::RealSource<'_>{
  sts2_native_obs::replay::RealSource{game:self.game(),
   obs:&self.obs,team:if self.team_active{self.team.as_deref()}else{None},
   decision:self.decision,seats:self.seats,inventory:self.inventory(),floor:self.floor()}
 }
 pub fn map(&self)->Value{self.projection().replay_map()}
 pub fn frame(&self,slot:usize,index:usize)->Result<Value,String>{
  self.projection().replay_frame(slot as i32,index)
 }
 pub fn queue_cursor(&self)->usize{self.projection().queue_cursor()}
 pub fn queue_events_since(&self,from:usize)->Result<Value,String>{self.projection().queue_events_since(from)}
 pub fn replay_snapshot(&self,actor:u8)->Result<Value,String>{self.projection().replay_snapshot(actor)}
 pub fn decision_input(&self)->Result<(NativeDecision,String),String>{
  let decision=NativeDecision::from_view(self.observation()?)?;
  let hash=sha256(decision.trajectory_json(self.relational).map_err(|e|e.to_string())?.as_bytes());
  Ok((decision,hash))
 }
}
