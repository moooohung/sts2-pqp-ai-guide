//! Solo original-input replay. GoldReward auto-collection and suspended
//! selectors are folded at settled boundaries, never repaired with recorded RNGs.
use serde_json::{Value,json};
use sts2_native_obs::*;
use sts2_native_obs::reward::{Reward,State,Outcome,solo_objective_complete};
use sts2_sim::run::{RunManager,RunOptions,RunPhase,NativeRewardContent};
use crate::{checkpoint,recording::Recording,party::{Failure,ReplayStep,RolloutSample,fail,index,require_training_ascension}};
type Result<T>=std::result::Result<T,Failure>;

pub(crate) fn fresh(r:&Recording)->Result<RunManager>{
    if !r.is_v2()||r.party.len()!=1{return Err(fail("SOLO_V2_REQUIRED"));}
    Ok(RunManager::from_numeric_seed(r.numeric_seed.unwrap(),RunOptions{
        ascension:r.payload["ascension"].as_i64().ok_or_else(||fail("INVALID_ASCENSION"))?
            .try_into().map_err(|_|fail("INVALID_ASCENSION"))?,
        number_of_runs:sts2_sim::run::mp::MpUnlockState::all().number_of_runs,
        ..RunOptions::default()
    },r.party[0]))
}
pub struct VerifiedSolo<'a>{recording:&'a Recording,steps:Vec<ReplayStep>,boundaries:usize,floors:usize}
impl VerifiedSolo<'_>{
    pub fn steps(&self)->&[ReplayStep]{&self.steps}
    pub fn checked_boundaries(&self)->usize{self.boundaries}
    pub fn checked_floors(&self)->usize{self.floors}
    /// `reward` is the trainer's own contract (training_config.reward); each
    /// transition is measured across the native step, as native_run does.
    pub fn rollout(&self,reward:&Reward,mut emit:impl FnMut(RolloutSample<'_>))->Result<()>{
        reward.validate().map_err(|_|fail("INVALID_REWARD_CONFIG"))?;
        let mut rewards=Vec::with_capacity(self.steps.len());
        {
            let mut game=fresh(self.recording)?;
            let mut encoder=NativeEncoder::new();let mut obs=Box::new(NativeObsV1::new());
            let mut decision=DecisionId{episode:self.recording.numeric_seed.unwrap(),generation:0,actor:0};
            for s in &self.steps{
                encoder.encode_solo(&game,decision,&mut obs).map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                let before=State::solo(&game);
                encoder.dispatch_solo(&mut game,&mut decision,&mut obs,s.candidate_slot)
                    .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                let outcome=s.terminal.then(||if solo_objective_complete(&game){Outcome::Won}else{Outcome::Death});
                let t=reward.transition_outcome(&before,&State::solo(&game),outcome);
                if !t.team_reward.is_finite(){return Err(fail("INVALID_REWARD_CONFIG"));}
                rewards.push(t.team_reward);
            }
        }
        let mut game=fresh(self.recording)?;
        let mut encoder=NativeEncoder::new();let mut obs=Box::new(NativeObsV1::new());
        let mut decision=DecisionId{episode:self.recording.numeric_seed.unwrap(),generation:0,actor:0};
        for (s,&reward) in self.steps.iter().zip(&rewards){
            encoder.encode_solo(&game,decision,&mut obs).map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
            let view=obs.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
            let mask:Vec<_>=(0..view.candidate_len).map(|i|view.candidates.sampleable(i,view.candidate_len)).collect();
            if !mask[s.candidate_slot]||obs.binding(s.candidate_slot,decision,1)
                .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?.command!=s.command {
                return Err(fail("VERIFIED_PLAN_REPLAY_FAILED"));
            }
            emit(RolloutSample{observation:view,position:mask[..s.candidate_slot].iter().filter(|b|**b).count(),
                legal_mask:mask,chosen_action_index:s.candidate_slot,reward,terminal:s.terminal,seat:0});
            encoder.dispatch_solo(&mut game,&mut decision,&mut obs,s.candidate_slot)
                .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
        }Ok(())
    }
}
struct Replay<'a>{
    live:bool,
    recording:&'a Recording,game:RunManager,encoder:NativeEncoder,obs:Box<NativeObsV1>,decision:DecisionId,
    history:Vec<Value>,group:usize,backings:Vec<Option<usize>>,done:Vec<bool>,finished:Vec<bool>,
    steps:Vec<ReplayStep>,boundaries:usize,floors:usize,seq:usize,inputs_done:bool,
}
impl<'a> Replay<'a>{
    fn check_before(&mut self)->Result<()>{
        let rows=self.recording.payload["actions"].as_array().unwrap();
        let row=&rows[self.seq];
        if self.game.phase()==RunPhase::Treasure&&row["action"]["kind"]=="Collect"&&
            row["action"]["reward_type"]=="relic"{
            // OneOffSynchronizer.cs:111-128 awards chest gold before the UI
            // offers the relic. Native Collect is atomic (gold + relic), so
            // use the independently checked room-entry boundary before opening
            // the chest, then the following input after the relic is obtained.
            let previous=self.seq.checked_sub(1).and_then(|i|rows.get(i))
                .ok_or_else(||fail("TREASURE_ENTRY_BOUNDARY_MISSING"))?;
            if previous["action"]["kind"]!="Move"||previous["after"]["replay"]["room"]!="Treasure"{
                return Err(fail("TREASURE_ENTRY_BOUNDARY_MISSING"));
            }
            checkpoint::compare(&checkpoint::project(&self.game.checkpoint_view(),&self.history,0)?,
                &previous["after"]["replay"])?;
        }else{
            checkpoint::compare(&checkpoint::project(&self.game.checkpoint_view(),&self.history,0)?,
                &row["before"])?;
        }
        self.boundaries+=1;Ok(())
    }
    fn new(r:&'a Recording)->Result<Self>{
        let game=fresh(r)?;
        crate::certification::initialization(r,&game.initialization_checkpoint_view()
            .map_err(|_|fail("INITIALIZATION_VIEW_UNAVAILABLE"))?)?;
        let n=r.payload["actions"].as_array().unwrap().len();
        Ok(Self{live:false,recording:r,game,encoder:NativeEncoder::new(),obs:Box::new(NativeObsV1::new()),
            decision:DecisionId{episode:r.numeric_seed.unwrap(),generation:0,actor:0},history:Vec::new(),
            group:0,backings:Vec::new(),done:vec![false;n],finished:vec![false;n],
            steps:Vec::new(),boundaries:0,floors:0,seq:0,inputs_done:false})
    }
    fn legal(&mut self)->Result<Vec<(usize,Binding)>>{
        self.encoder.encode_solo(&self.game,self.decision,&mut self.obs).map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        let v=self.obs.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        if v.status!=DecisionStatus::Actionable{return Err(fail("ORIGINAL_INPUT_HAS_NO_NATIVE_DECISION"));}
        (0..v.candidate_len).filter(|&i|v.candidates.sampleable(i,v.candidate_len))
            .map(|i|self.obs.binding(i,self.decision,1).map(|b|(i,b))
                .map_err(|_|fail("NATIVE_BINDING_FAILED"))).collect()
    }
    fn execute(&mut self,c:SoloCommand)->Result<()>{
        self.execute_offer(c,None)
    }
    fn execute_offer(&mut self,c:SoloCommand,offer:Option<u64>)->Result<()>{
        // Multi-card selection prefixes only update selector bookkeeping.
        // Every emitted native decision must still match an original boundary.
        self.check_before()?;
        let command=Command::Solo(c);
        let slots:Vec<_>=self.legal()?.into_iter().filter(|(_,b)|b.command==command&&
            offer.is_none_or(|o|b.offer_instance==Some(o))).collect();
        if slots.len()!=1{return Err(fail("ORIGINAL_INPUT_HAS_NO_EXACT_NATIVE_ACTION"));}
        let slot=slots[0].0;
        self.encoder.dispatch_solo(&mut self.game,&mut self.decision,&mut self.obs,slot)
            .map_err(|_|fail("NATIVE_ACTION_REJECTED"))?;
        self.steps.push(ReplayStep{actor:0,command,candidate_slot:slot,source_seq:self.seq,forced:false,
            floor:self.game.run_ctx().total_floor.max(0) as u32,terminal:self.game.phase()==RunPhase::RunOver});
        Ok(())
    }
    fn prepare(&mut self,expected:&Value)->Result<()>{
        let all=expected.as_array().ok_or_else(||fail("MISSING_REWARD_CHECKPOINT"))?;
        if all.len()<self.history.len(){return Err(fail("REWARD_HISTORY_SHRANK"));}
        if all.len()==self.history.len(){return Ok(());}
        let mut offers=Vec::new();let mut backings=Vec::new();
        if self.game.phase()==RunPhase::Treasure{
            return Err(fail("TREASURE_REWARD_LEDGER_UNSUPPORTED"));
        }else{
            for &gold in self.game.checkpoint_generated_gold(){
                offers.push(json!({"kind":"GoldReward","selected":false,"cards":[],"gold":gold}));backings.push(None);
            }
            for i in self.game.native_reward_indices(){
                let reward=self.game.native_reward(i).unwrap();
                let offer=match reward.content{
                    NativeRewardContent::Cards{cards,..}=>json!({"kind":"CardReward","selected":false,
                        "cards":cards.iter().map(checkpoint::card).collect::<std::result::Result<Vec<_>,_>>()?}),
                    NativeRewardContent::Potion(p)=>json!({"kind":"PotionReward","selected":false,"cards":[],
                        "model":checkpoint::potion_name(p)}),
                    NativeRewardContent::Relic(r)=>json!({"kind":"RelicReward","selected":false,"cards":[],"model":r.name()}),
                    _=>return Err(fail("UNSUPPORTED_REWARD_OFFER")),
                };
                offers.push(offer);backings.push(Some(i));
            }
        }
        checkpoint::compare(&Value::Array(offers.clone()),&Value::Array(all[self.history.len()..].to_vec()))?;
        self.group=self.history.len();self.history.extend(offers);self.backings=backings;Ok(())
    }
    fn display(&self,original:usize)->Result<u32>{
        let backing=self.backings.get(original).copied().flatten().ok_or_else(||fail("INVALID_REWARD_INDEX"))?;
        self.game.native_reward_indices().position(|i|i==backing).map(|i|i as u32)
            .ok_or_else(||fail("REWARD_ALREADY_CONSUMED"))
    }
    fn gold_pending(&self)->bool{
        self.history.iter().any(|r|r["kind"]=="GoldReward"&&r["selected"]==false)
    }
    fn state(&self)->Result<Value>{Ok(checkpoint::project_state(&self.game.checkpoint_view(),&self.history,0)?)}
    fn floor(&mut self)->Result<()>{
        if self.live {return Ok(());}
        let all=self.recording.payload["floor_digests"].as_array().unwrap();
        let floor=self.game.run_ctx().total_floor;
        if self.floors>0&&all[self.floors-1]["floor"].as_i64()==Some(floor as i64){return Ok(());}
        let row=all.get(self.floors).ok_or_else(||fail("MISSING_FLOOR_CERTIFICATE"))?;
        if row["floor"].as_i64()!=Some(floor as i64)||
            row["act"].as_i64()!=Some(self.game.run_ctx().act_index as i64+1)||
            row["after_action"].as_i64()!=Some(self.seq as i64-1){
            return Err(fail("FLOOR_CERTIFICATE_ORDER_MISMATCH"));
        }
        self.prepare(&row["state"]["replay"]["rewards"])?;
        checkpoint::compare(&self.state()?,&row["state"])?;self.floors+=1;Ok(())
    }
    fn finish(&mut self,mut seq:usize)->Result<()>{
        self.done[seq]=true;
        loop{
            let rows=self.recording.payload["actions"].as_array().unwrap();
            if rows.iter().enumerate().any(|(i,r)|r["parent_seq"].as_u64()==Some(seq as u64)&&!self.finished[i]){break;}
            let row=&rows[seq];
            if !self.live&&row["parent_seq"].is_null()&&row["action"]["kind"]!="EndTurn"&&!self.gold_pending()&&
                !(self.game.checkpoint_view().combat.is_some_and(|c|c.is_over)&&row["action"]["kind"]=="PlayCard")&&
                !(row["action"]["kind"]=="Collect"&&row["action"]["reward_type"]=="relic"){
                self.prepare(&row["after"]["replay"]["rewards"])?;
                checkpoint::compare(&self.state()?,&row["after"])?;self.boundaries+=1;self.floor()?;
            }
            self.finished[seq]=true;
            let Some(parent)=row["parent_seq"].as_u64() else{break};
            seq=parent as usize;if !self.done[seq]{break;}
        }Ok(())
    }
    fn drive(&mut self)->Result<()>{
        for seq in 0..self.done.len(){
            if self.live && self.done[seq] {continue;}
            self.seq=seq;let row=&self.recording.payload["actions"][seq];let a=&row["action"];
            self.prepare(&row["before"]["rewards"])?;
            self.check_before()?;self.floor()?;
            match a["kind"].as_str().unwrap(){
                "EventChoice"=>{
                    let v=self.game.checkpoint_view();
                    let option=v.event.as_ref().and_then(|e|e.options.get(index(&a["index"]).ok()? as usize))
                        .ok_or_else(||fail("INVALID_EVENT_OPTION"))?.identity.id;
                    if checkpoint::project(&v,&self.history,0)?["event_options"][index(&a["index"])? as usize]["id"]!=a["option_id"]{
                        return Err(fail("EVENT_OPTION_IDENTITY_MISMATCH"));
                    }
                    self.execute_offer(SoloCommand::EventChoice{option},Some(index(&a["index"])? as u64))?;
                }
                "PickCardBundle"=>{
                    let i=index(&a["index"])? as usize;
                    let mut matched=false;
                    for j in self.game.native_reward_indices(){
                        if let NativeRewardContent::Bundles(b)=self.game.native_reward(j).unwrap().content{
                            let cards=b.get(i).ok_or_else(||fail("INVALID_BUNDLE_INDEX"))?;
                            checkpoint::compare(&json!(cards.iter().map(checkpoint::card)
                                .collect::<std::result::Result<Vec<_>,_>>()?),&a["cards"])?;matched=true;
                        }
                    }
                    if !matched{return Err(fail("MISSING_NATIVE_BUNDLE"));}
                    self.execute(SoloCommand::PickCardBundle{index:i as u32})?;
                }
                "Move"=>self.execute(SoloCommand::Move{
                    x:i32::try_from(a["coord"][0].as_i64().ok_or_else(||fail("INVALID_MAP_COORD"))?)
                        .map_err(|_|fail("INVALID_MAP_COORD"))?,
                    y:i32::try_from(a["coord"][1].as_i64().ok_or_else(||fail("INVALID_MAP_COORD"))?)
                        .map_err(|_|fail("INVALID_MAP_COORD"))?})?,
                "PlayCard"=>{
                    let hand=index(&a["hand_index"])?;
                    let v=self.game.checkpoint_view();let c=v.combat.ok_or_else(||fail("MISSING_NATIVE_COMBAT"))?;
                    let k=*c.players[0].hand.get(hand as usize).ok_or_else(||fail("INVALID_HAND_INDEX"))?;
                    checkpoint::compare(&checkpoint::card(&c.cards[k as usize])?,&a["card"])?;
                    let target=if a["target"].is_null(){None}else{
                        Some(u16::try_from(index(&a["target"]["index"])?).map_err(|_|fail("INVALID_TARGET_INDEX"))?)
                    };
                    self.execute(SoloCommand::PlayCard{hand,target})?;
                }
                "EndTurn"=>self.execute(SoloCommand::EndTurn)?,
                "Skip" if self.live=>self.execute(SoloCommand::Skip)?,
                "SkipRewards" if self.live=>self.execute(SoloCommand::SkipRewards)?,
                "SkipPotion" if self.live=>self.execute(SoloCommand::SkipPotion)?,
                "SkipRelic" if self.live=>self.execute(SoloCommand::SkipRelic)?,
                "OpenReward"=>{
                    let n=index(&a["reward_index"])? as usize;let display=self.display(n)?;
                    let backing=self.backings[n].unwrap();
                    if !self.game.native_reward(backing).unwrap().active {
                        self.execute(SoloCommand::OpenReward{reward_index:display})?;
                    }
                }
                "PickCard"=>{
                    if a["reward_type"]!="card"{return Err(fail("CARD_REWARD_KIND_MISMATCH"));}
                    let r=self.group+index(&a["reward_index"])? as usize;
                    let reward=self.history.get(r).ok_or_else(||fail("INVALID_REWARD_INDEX"))?;
                    checkpoint::compare(&reward["cards"],&a["offered_cards"])?;
                    let i=index(&a["index"])?;
                    self.execute(SoloCommand::PickCard{index:i})?;
                    self.history[r]["cards"].as_array_mut().unwrap().remove(i as usize);
                    self.history[r]["selected"]=true.into();
                }
                "Collect" if self.game.phase()==RunPhase::Treasure=>{
                    let id=self.game.native_treasure_relic().ok_or_else(||fail("MISSING_TREASURE_OFFER"))?;
                    if json!(id.name())!=a["relic_id"]||a["reward_index"]!=0{
                        return Err(fail("TREASURE_IDENTITY_MISMATCH"));
                    }
                    self.execute(SoloCommand::Collect)?;
                }
                "Collect" if a["reward_type"]=="gold"=>{
                    let r=self.group+index(&a["reward_index"])? as usize;
                    if self.history.get(r).is_none_or(|o|o["kind"]!="GoldReward"||o["selected"]!=false){
                        return Err(fail("INVALID_GOLD_COLLECTION"));
                    }
                    self.history[r]["selected"]=true.into();
                }
                "PickPotion"=>{
                    let n=index(&a["reward_index"])? as usize;
                    let backing=self.backings.get(n).copied().flatten().ok_or_else(||fail("INVALID_REWARD_INDEX"))?;
                    let reward=self.game.native_reward(backing).ok_or_else(||fail("MISSING_POTION_REWARD"))?;
                    if self.history[self.group+n]["model"]!=a["potion_id"]{return Err(fail("POTION_IDENTITY_MISMATCH"));}
                    self.execute(SoloCommand::PickPotion{reward_index:if reward.active{None}else{Some(self.display(n)?)}})?;
                    self.history[self.group+n]["selected"]=true.into();
                }
                "UsePotion"=>{
                    let slot=index(&a["slot_index"])?;
                    let p=self.game.run_ctx().potions.get(slot as usize).copied().flatten().ok_or_else(||fail("MISSING_POTION"))?;
                    if json!(checkpoint::potion_name(p))!=a["potion_id"]{return Err(fail("POTION_IDENTITY_MISMATCH"));}
                    let commands:Vec<_>=self.legal()?.into_iter().filter_map(|(_,b)|match b.command {
                        Command::Solo(c@SoloCommand::UsePotion{slot:s,..}) if s==slot=>Some(c),_=>None}).collect();
                    if commands.len()!=1||a["target"]["side"]!="player"||a["target"]["index"]!=0||
                        !matches!(commands[0],SoloCommand::UsePotion{target:PotionRecipient::None|PotionRecipient::Player(0),..}){
                        return Err(fail("POTION_TARGET_UNSUPPORTED"));
                    }
                    self.execute(commands[0])?;
                }
                "Choose"=>{
                    let indices=a["indices"].as_array().ok_or_else(||fail("MISSING_SELECTOR_INDICES"))?;
                    for (i,n) in indices.iter().enumerate(){
                        let n=index(n)?;
                        if let Some(choice)=self.game.native_choice(){
                            let card=choice.card(n as usize).ok_or_else(||fail("INVALID_SELECTOR_INDEX"))?.card;
                            checkpoint::compare(&checkpoint::card(card)?,&a["cards"][i])?;
                        }
                        self.execute(SoloCommand::Choose{index:n})?;
                    }
                    if self.game.native_choice().is_some_and(|c|c.can_confirm())||
                        (indices.is_empty()&&self.game.phase()==RunPhase::Combat){
                        // PowerPotion.cs:29 permits an empty ChooseACard
                        // result; native combat selectors expose that as
                        // ConfirmChoice, not the reward-menu Skip command.
                        self.execute(SoloCommand::ConfirmChoice)?;
                    }else if indices.is_empty(){self.execute(SoloCommand::Skip)?;}
                }
                _=>return Err(fail("ORIGINAL_INPUT_FOLD_UNSUPPORTED")),
            }
            self.finish(seq)?;
        }
        self.inputs_done=true;
        if self.live {return Ok(());}
        if self.finished.iter().any(|b|!*b)||self.floors!=self.recording.payload["floor_digests"].as_array().unwrap().len(){
            return Err(fail("UNCONSUMED_ORIGINAL_INPUT"));
        }
        if self.game.phase()!=RunPhase::RunOver||self.game.observation_full_run_victory()||
            self.recording.payload["result"]["won"]!=false{
            return Err(fail("NATIVE_TERMINAL_DIVERGENCE"));
        }
        checkpoint::compare(&self.state()?,&self.recording.payload["result"]["terminal_state"])?;
        self.boundaries+=1;Ok(())
    }
}
/// Drives every original input; the replay (when one started) keeps the native steps
/// reproduced before the first divergence.
fn drive_all(recording:&Recording)->(Option<Replay<'_>>,Result<()>){
    if crate::recording::validate_parents(&recording.payload).is_err(){
        return (None,Err(fail("INVALID_PARENT_SEQUENCE")));
    }
    if let Err(e)=crate::state_hash::validate(recording){return (None,Err(e.into()));}
    let mut replay=match Replay::new(recording){Ok(replay)=>replay,Err(e)=>return (None,Err(e))};
    let result=replay.drive().map_err(|mut e|{if e.seq.is_none(){e.seq=Some(replay.seq);}e});
    (Some(replay),result)
}
pub fn verify(recording:&Recording)->Result<VerifiedSolo<'_>>{
    require_training_ascension(recording)?;
    let (replay,result)=drive_all(recording);
    result?;
    let replay=replay.expect("a fully driven recording has a replay");
    Ok(VerifiedSolo{recording,steps:replay.steps,boundaries:replay.boundaries,floors:replay.floors})
}
pub(crate) fn reproduce(recording:&Recording)->crate::Reproduction{
    let (replay,result)=drive_all(recording);
    let (steps,inputs_done)=replay.map_or((Vec::new(),false),|r|(r.steps,r.inputs_done));
    crate::Reproduction{steps,ops:Vec::new(),inputs_done,failure:result.err()}
}

fn live_replay(recording:&Recording,encoder:NativeEncoder)->Result<Replay<'_>>{
    let mut replay=Replay::new(recording)?;
    replay.live=true;
    replay.encoder=encoder;
    replay.drive().map_err(|mut e|{if e.seq.is_none(){e.seq=Some(replay.seq);}e})?;
    Ok(replay)
}

pub(crate) fn live_probe_steps(recording:&Recording,probes:&[Recording],
    encoder:impl Fn()->NativeEncoder)->Result<Vec<Vec<ReplayStep>>>{
    let base=live_replay(recording,encoder())?;
    let count=base.steps.len();
    probes.iter().map(|recording|{
        let n=recording.payload["actions"].as_array().unwrap().len();
        let mut done=base.done.clone();done.resize(n,false);
        let mut finished=base.finished.clone();finished.resize(n,false);
        let mut replay=Replay{live:true,recording,game:base.game.clone(),encoder:encoder(),
            obs:Box::new(NativeObsV1::new()),decision:base.decision,
            history:base.history.clone(),group:base.group,backings:base.backings.clone(),done,finished,
            steps:base.steps.clone(),boundaries:base.boundaries,floors:base.floors,seq:base.seq,inputs_done:false};
        replay.drive().map_err(|mut e|{if e.seq.is_none(){e.seq=Some(replay.seq);}e})?;
        Ok(replay.steps.into_iter().skip(count).collect())
    }).collect()
}

pub(crate) fn live_pending(recording:&Recording,encoder:NativeEncoder)->Result<(Vec<crate::live::Pending>,crate::live::LiveRun)>{
    let mut replay=live_replay(recording,encoder)?;
    replay.encoder.encode_solo(&replay.game,replay.decision,&mut replay.obs)
        .map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
    let pending=vec![crate::live::Pending::new(replay.obs,replay.decision,1)?];
    Ok((pending,crate::live::LiveRun::Solo(replay.game)))
}
