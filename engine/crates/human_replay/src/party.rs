//! Production original-input party replay. No samples are admitted on a
//! missing boundary, unsupported fold, illegal command, or state divergence.
use serde_json::{Value,json};
use sts2_native_obs::*;
use sts2_native_obs::reward::{Reward,State,Outcome,credit,party_objective_complete};
use sts2_sim::{combat::Combat,run::{mp::{MpRun,MpPhase,PlayerSpec},replay_view::ReplayCheckpointView}};
use crate::{recording::Recording,checkpoint::{self,Mismatch},folding::{self,CardIdentity,CardRewardChoice}};

#[derive(Debug,Clone)]
pub struct Failure {pub code:&'static str,pub seq:Option<usize>,pub path:String}
impl From<Mismatch> for Failure {
    fn from(e:Mismatch)->Self{Self{code:e.code,seq:None,path:e.path}}
}
type Result<T>=std::result::Result<T,Failure>;
pub(crate) fn fail(code:&'static str)->Failure{Failure{code,seq:None,path:String::new()}}
/// Replays are viewable at A0-A10; only explicit A10 recordings may become training samples.
pub(crate) fn require_training_ascension(recording:&Recording)->Result<()>{
    if recording.payload["ascension"].as_u64()==Some(10) {Ok(())}
    else {Err(Failure{code:"TRAINING_ASCENSION_REQUIRED",seq:None,path:"ascension".into()})}
}
pub(crate) fn index(v:&Value)->Result<u32>{
    v.as_u64().and_then(|n|u32::try_from(n).ok()).ok_or_else(||fail("INVALID_ORIGINAL_INDEX"))
}
fn identity(v:&Value)->Result<CardIdentity>{
    Ok(CardIdentity{id:v["id"].as_str().ok_or_else(||fail("MISSING_CARD_ID"))?.to_owned(),
        upgrade:index(&v["upgrade"])?})
}
fn from_fold(e:folding::FoldError)->Failure{fail(e.code())}

#[derive(Debug,Clone)]
pub struct ReplayStep {
    pub actor:u8,pub command:Command,pub candidate_slot:usize,pub source_seq:usize,
    pub forced:bool,pub floor:u32,pub terminal:bool,
}
/// Borrowed NativeObsV1 view accepted directly by trainer NativeInput::pack_view.
/// `chosen_action_index` indexes the original candidates; `position` indexes
/// the compact legal slots, as native_run::Sample.position does.
pub struct RolloutSample<'a> {
    pub observation:ObservationView<'a>,pub legal_mask:Vec<bool>,
    pub chosen_action_index:usize,pub position:usize,
    pub reward:f32,pub terminal:bool,pub seat:u8,
}
#[derive(Debug,Clone)]
pub(crate) enum Op {Advance,Action(usize)}
pub struct VerifiedParty<'a> {
    recording:&'a Recording,steps:Vec<ReplayStep>,ops:Vec<Op>,boundaries:usize,floors:usize,
}
impl VerifiedParty<'_> {
    pub fn steps(&self)->&[ReplayStep]{&self.steps}
    pub fn checked_boundaries(&self)->usize{self.boundaries}
    pub fn checked_floors(&self)->usize{self.floors}
    pub fn recording(&self)->&Recording{self.recording}
    /// `reward` is the trainer's own contract (training_config.reward).
    /// Mirrors native_run: each action's transition runs through the native
    /// automatic advancement that follows it, seat rewards go to each seat's
    /// latest sample (reward::credit), a new sample takes its seat's pending
    /// credit, and each seat's final sample is terminal.
    /// No observation is emitted before this entire run has been verified.
    pub fn rollout(&self,reward:&Reward,
        mut emit:impl FnMut(RolloutSample<'_>))->Result<()> {
        reward.validate().map_err(|_|fail("INVALID_REWARD_CONFIG"))?;
        let seats=self.recording.party.len();
        let mut rewards=vec![0f32;self.steps.len()];
        let mut terminal=vec![false;self.steps.len()];
        let mut last=vec![None;seats];
        let mut pending=vec![0f32;seats];
        {
            let mut game=fresh(self.recording)?;
            let mut encoder=NativeEncoder::new();
            let mut obs=Box::new(NativeObsV1::new());
            let mut decision=DecisionId{episode:self.recording.numeric_seed.unwrap(),generation:0,actor:0};
            let mut open:Option<(usize,State)>=None;
            let settle=|i:usize,before:&State,game:&MpRun,rewards:&mut [f32],last:&[Option<usize>],pending:&mut [f32]|->Result<()>{
                let s=&self.steps[i];
                let outcome=s.terminal.then(||if party_objective_complete(game){Outcome::Won}else{Outcome::Death});
                let t=reward.transition_outcome(before,&State::party(game),outcome);
                if !t.team_reward.is_finite(){return Err(fail("INVALID_REWARD_CONFIG"));}
                credit(&t.seat_rewards[..seats],last.iter().copied(),pending,|k,r|rewards[k]+=r);
                Ok(())
            };
            for op in &self.ops {
                match *op {
                    Op::Advance=>{NativeEncoder::advance_multiplayer(&mut game,&mut decision)
                        .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;}
                    Op::Action(i)=>{
                        if let Some((k,before))=open.take(){settle(k,&before,&game,&mut rewards,&last,&mut pending)?;}
                        let s=&self.steps[i];decision.actor=s.actor;
                        let net=s.actor as u64+1;
                        encoder.encode_multiplayer(&game,decision,net,&mut obs)
                            .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                        let actor=s.actor as usize;
                        rewards[i]+=std::mem::take(&mut pending[actor]);last[actor]=Some(i);
                        open=Some((i,State::party(&game)));
                        encoder.dispatch_multiplayer(&mut game,&mut decision,net,&mut obs,s.candidate_slot)
                            .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                    }
                }
            }
            if let Some((k,before))=open.take(){settle(k,&before,&game,&mut rewards,&last,&mut pending)?;}
        }
        if pending.iter().any(|r|*r!=0.) {return Err(fail("UNASSIGNED_SEAT_REWARD"));}
        for i in last.into_iter().flatten(){terminal[i]=true;}
        let mut game=fresh(self.recording)?;
        let mut encoder=NativeEncoder::new();
        let mut obs=Box::new(NativeObsV1::new());
        let mut decision=DecisionId{episode:self.recording.numeric_seed.unwrap(),generation:0,actor:0};
        for op in &self.ops {
            match *op {
                Op::Advance=>{NativeEncoder::advance_multiplayer(&mut game,&mut decision)
                    .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;}
                Op::Action(i)=>{
                    let s=&self.steps[i];decision.actor=s.actor;
                    let net=s.actor as u64+1;
                    encoder.encode_multiplayer(&game,decision,net,&mut obs)
                        .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                    let view=obs.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
                    let mask:Vec<_>=(0..view.candidate_len).map(|k|
                        view.candidates.sampleable(k,view.candidate_len)).collect();
                    let binding=obs.binding(s.candidate_slot,decision,net)
                        .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                    if !mask.get(s.candidate_slot).copied().unwrap_or(false)||binding.command!=s.command {
                        return Err(fail("VERIFIED_PLAN_REPLAY_FAILED"));
                    }
                    emit(RolloutSample{observation:view,position:mask[..s.candidate_slot].iter().filter(|b|**b).count(),
                        legal_mask:mask,chosen_action_index:s.candidate_slot,reward:rewards[i],
                        terminal:terminal[i],seat:s.actor});
                    encoder.dispatch_multiplayer(&mut game,&mut decision,net,&mut obs,s.candidate_slot)
                        .map_err(|_|fail("VERIFIED_PLAN_REPLAY_FAILED"))?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Group {actor:usize,floor:i32,pending:bool,start:usize,end:usize}
#[derive(Default,Clone)]
struct Ledger {values:Vec<Value>,groups:Vec<Group>}
impl Ledger {
    fn prepare(&mut self,run:&MpRun,actor:usize,expected:&Value)->Result<()> {
        let all=expected.as_array().ok_or_else(||fail("MISSING_REWARD_CHECKPOINT"))?;
        if all.len()<self.values.len(){return Err(fail("REWARD_HISTORY_SHRANK"));}
        if all.len()==self.values.len(){return Ok(());}
        let floor=run.world.run.total_floor;
        let mut group=Vec::new();
        let pending;
        if let Some(reward)=run.relic_rewards[actor] {
            pending=true;
            group.push(json!({"kind":"RelicReward","selected":false,"cards":[],"model":reward.name()}));
        }else if let Some(rewards)=&run.rewards {
            pending=false;
            let r=&rewards[actor];
            if !r.extra_cards.is_empty()||!r.extra_potions.is_empty()||!r.extra_relics.is_empty()||
                !r.special_cards.is_empty(){return Err(fail("UNSUPPORTED_EXTRA_REWARD_ORDER"));}
            for &amount in &r.gold {group.push(json!({"kind":"GoldReward","selected":false,"cards":[],"gold":amount}));}
            if let Some(p)=r.potion {
                group.push(json!({"kind":"PotionReward","selected":false,"cards":[],"model":checkpoint::potion_name(p)}));
            }
            if !r.cards.is_empty() {
                group.push(json!({"kind":"CardReward","selected":false,
                    "cards":r.cards.iter().map(checkpoint::card).collect::<std::result::Result<Vec<_>,_>>()?}));
            }
        }else{return Err(fail("UNREPRESENTABLE_REWARD_OFFER"));}
        if self.groups.iter().any(|g|g.actor==actor&&g.floor==floor&&g.pending==pending) {
            return Err(fail("AMBIGUOUS_REWARD_GROUP"));
        }
        let mut native=Value::Array(group.clone());checkpoint::normalize(&mut native);
        checkpoint::compare(&native,&Value::Array(all[self.values.len()..].to_vec()))?;
        let start=self.values.len();self.values.extend(group);
        self.groups.push(Group{actor,floor,pending,start,end:self.values.len()});
        Ok(())
    }
    fn apply(&mut self,actor:usize,command:Command)->Result<()> {
        let pending=matches!(command,Command::MpRelicReward{..});
        if !matches!(command,Command::MpTakeCard{..}|Command::MpTakeGoldSkipRemaining|Command::MpRelicReward{..}) {
            return Ok(());
        }
        let g=self.groups.iter().rev().find(|g|g.actor==actor&&g.pending==pending)
            .ok_or_else(||fail("REWARD_SELECTION_WITHOUT_OFFER"))?;
        let group=&mut self.values[g.start..g.end];
        match command {
            Command::MpTakeCard{index}=>{
                let r=group.iter_mut().find(|r|r["kind"]=="CardReward"&&r["selected"]==false)
                    .ok_or_else(||fail("CARD_REWARD_ALREADY_CONSUMED"))?;
                let cards=r["cards"].as_array_mut().ok_or_else(||fail("MISSING_CARD_OFFER"))?;
                if index as usize>=cards.len(){return Err(fail("CARD_REWARD_INDEX_OUT_OF_RANGE"));}
                cards.remove(index as usize);r["selected"]=true.into();
            }
            Command::MpTakeGoldSkipRemaining=>{
                for r in group {if r["kind"]=="GoldReward"{r["selected"]=true.into();}}
            }
            Command::MpRelicReward{accept:true}=>{
                for r in group {if r["kind"]=="RelicReward"{r["selected"]=true.into();}}
            }
            _=>return Err(fail("UNSUPPORTED_REWARD_SKIP")),
        }
        Ok(())
    }
}

pub(crate) fn fresh(recording:&Recording)->Result<MpRun>{
    if !recording.is_v2(){return Err(fail("V1_READ_ONLY_NOT_NATIVE_TRAINING"));}
    if !(2..=4).contains(&recording.party.len()){return Err(fail("PARTY_SIZE_UNSUPPORTED"));}
    let specs:Vec<_>=recording.party.iter().enumerate().map(|(seat,&character)|
        PlayerSpec{net_id:seat as u64+1,character}).collect();
    MpRun::from_numeric_seed(recording.numeric_seed.ok_or_else(||fail("MISSING_NUMERIC_SEED"))?,
        &specs,recording.payload["ascension"].as_i64().ok_or_else(||fail("INVALID_ASCENSION"))?
            .try_into().map_err(|_|fail("INVALID_ASCENSION"))?)
        .map_err(|_|fail("NATIVE_INITIALIZATION_FAILED"))
}
fn view<'a>(run:&'a MpRun,closed:&'a Option<Box<Combat>>,local:usize)->Result<ReplayCheckpointView<'a>>{
    let mut v=run.checkpoint_view(local).map_err(|_|fail("NATIVE_CHECKPOINT_UNAVAILABLE"))?;
    if v.combat.is_none()&&matches!(v.run.room_type,sts2_sim::tables::RoomType::Monster|
        sts2_sim::tables::RoomType::Elite|sts2_sim::tables::RoomType::Boss) {
        v.combat=closed.as_deref();
        v.player_combat=v.combat;
    }
    Ok(v)
}

struct Replay<'a> {
    live:bool,
    recording:&'a Recording,run:MpRun,encoder:NativeEncoder,obs:Box<NativeObsV1>,decision:DecisionId,
    closed:Option<Box<Combat>>,ledger:Ledger,steps:Vec<ReplayStep>,ops:Vec<Op>,
    processed:Vec<bool>,finished:Vec<bool>,children:Vec<Vec<usize>>,boundaries:usize,local:usize,current_seq:usize,floors:usize,
    inputs_done:bool,
}
impl<'a> Replay<'a> {
    fn new(recording:&'a Recording)->Result<Self>{
        let count=recording.payload["actions"].as_array().ok_or_else(||fail("MISSING_ACTIONS"))?.len();
        let mut children=vec![Vec::new();count];
        for (i,row) in recording.payload["actions"].as_array().unwrap().iter().enumerate() {
            if let Some(parent)=row["parent_seq"].as_u64(){children[parent as usize].push(i);}
        }
        Ok(Self{live:false,recording,run:fresh(recording)?,encoder:NativeEncoder::new(),obs:Box::new(NativeObsV1::new()),
            decision:DecisionId{episode:recording.numeric_seed.unwrap(),generation:0,actor:0},closed:None,ledger:Ledger::default(),
            steps:Vec::new(),ops:Vec::new(),processed:vec![false;count],finished:vec![false;count],
            children,boundaries:0,local:recording.payload["local_seat"].as_u64().unwrap() as usize,current_seq:0,floors:0,
            inputs_done:false})
    }
    fn certify_floor(&mut self)->Result<()> {
        if self.live {return Ok(());}
        let rows=self.recording.payload["floor_digests"].as_array()
            .ok_or_else(||fail("MISSING_FLOOR_CERTIFICATE"))?;
        let floor=self.run.world.run.total_floor;
        if self.floors>0&&rows[self.floors-1]["floor"].as_i64()==Some(floor as i64) {
            return Ok(());
        }
        let row=rows.get(self.floors).ok_or_else(||fail("MISSING_FLOOR_CERTIFICATE"))?;
        if row["floor"].as_i64()!=Some(floor as i64)||
            row["act"].as_i64()!=Some(self.run.world.run.act_index as i64+1) {
            return Err(fail("FLOOR_CERTIFICATE_ORDER_MISMATCH"));
        }
        // Floor capture runs inside the entering top-level input, before that
        // row is appended by LiveRunSession.Add; no input may be reordered.
        if row["after_action"].as_i64()!=Some(self.current_seq as i64-1) {
            return Err(fail("FLOOR_ACTION_INDEX_MISMATCH"));
        }
        let entry=if self.run.combat().is_some() {
            Some(self.run.room_entry_checkpoint_combat().map_err(|_|fail("ROOM_ENTRY_VIEW_UNAVAILABLE"))?)
        }else{None};
        let mut v=view(&self.run,&self.closed,self.local)?;
        if let Some(entry)=&entry {
            v.run=&entry.run;v.combat=Some(entry);v.player_combat=Some(entry);
            for (seat,p) in v.seats.iter_mut().enumerate() {
                p.hp=entry.cr(seat as u8).hp;p.max_hp=entry.cr(seat as u8).max_hp;
                p.relics=entry.run.relics.iter().filter(|r|r.owner==seat as u8).collect();
            }
        }
        let state=checkpoint::project_state(&v,&self.ledger.values,self.local)?;
        checkpoint::compare(&state,&row["state"])?;
        self.floors+=1;Ok(())
    }
    fn advance(&mut self)->Result<()> {
        if self.run.phase==MpPhase::Rewards&&self.run.rewards.is_none() {
            self.closed=self.run.combat().map(|c|Box::new(c.clone()));
        }
        NativeEncoder::advance_multiplayer(&mut self.run,&mut self.decision)
            .map_err(|_|fail("NATIVE_AUTOMATIC_EFFECT_FAILED"))?;
        self.ops.push(Op::Advance);Ok(())
    }
    fn before(&mut self,seq:usize,actor:usize)->Result<()> {
        let expected=&self.recording.payload["actions"][seq]["before"];
        if !expected.is_object(){return Err(fail("MISSING_NATIVE_BOUNDARY_CHECKPOINT"));}
        self.ledger.prepare(&self.run,actor,&expected["rewards"])?;
        let projected=checkpoint::project(&view(&self.run,&self.closed,self.local)?,&self.ledger.values,self.local)?;
        checkpoint::compare(&projected,expected)?;self.boundaries+=1;
        self.certify_floor()?;Ok(())
    }
    fn execute(&mut self,seq:usize,actor:usize,command:Command,forced:bool)->Result<()> {
        if self.steps.len()>=100_000{return Err(fail("NATIVE_SAMPLE_LIMIT"));}
        self.decision.actor=actor as u8;
        let net=actor as u64+1;
        self.encoder.encode_multiplayer(&self.run,self.decision,net,&mut self.obs)
            .map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        let v=self.obs.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        if v.status!=DecisionStatus::Actionable{return Err(fail("ORIGINAL_INPUT_HAS_NO_NATIVE_DECISION"));}
        let legal:Vec<_>=(0..v.candidate_len).filter(|&i|v.candidates.sampleable(i,v.candidate_len)).collect();
        let matches:Vec<_>=legal.iter().copied().filter(|&i|
            self.obs.binding(i,self.decision,net).is_ok_and(|b|b.command==command)).collect();
        if matches.len()!=1{return Err(fail("ORIGINAL_INPUT_HAS_NO_EXACT_NATIVE_ACTION"));}
        if forced&&legal.len()!=1{return Err(fail("AUTOMATIC_NATIVE_ACTION_IS_NOT_FORCED"));}
        let slot=matches[0];
        self.encoder.dispatch_multiplayer(&mut self.run,&mut self.decision,net,&mut self.obs,slot)
            .map_err(|_|fail("NATIVE_ACTION_REJECTED"))?;
        self.ledger.apply(actor,command)?;
        self.ops.push(Op::Action(self.steps.len()));
        self.steps.push(ReplayStep{actor:actor as u8,command,candidate_slot:slot,source_seq:seq,forced,
            floor:self.run.world.run.total_floor.max(0) as u32,terminal:self.run.phase==MpPhase::RunOver});
        Ok(())
    }
    fn finish(&mut self,mut seq:usize)->Result<()> {
        self.processed[seq]=true;
        loop {
            if self.children[seq].iter().any(|&c|!self.finished[c]){break;}
            let row=&self.recording.payload["actions"][seq];
            // Selector-return snapshots precede their suspended parent resuming;
            // the top-level parent's completion is the matching native boundary.
            // A lethal play can return before original AfterCombatVictory
            // continuations (e.g. BurningBlood.cs:18-24) finish. Native dispatch
            // already runs these effects. The next input/terminal checkpoint
            // is the settled boundary for that folded transition.
            if !self.live&&row["parent_seq"].is_null()&&row["action"]["kind"]!="EndTurn"&&
                !self.run.combat().is_some_and(|c|c.is_over) {
                let projected=checkpoint::project_state(&view(&self.run,&self.closed,self.local)?,
                    &self.ledger.values,self.local)?;
                checkpoint::compare(&projected,&row["after"]).map_err(|e|{
                    let mut e:Failure=e.into();e.seq=Some(seq);e
                })?;self.boundaries+=1;
                self.certify_floor()?;
            }
            self.finished[seq]=true;
            let Some(parent)=row["parent_seq"].as_u64() else{break};
            seq=parent as usize;
            if !self.processed[seq]{break;}
        }
        Ok(())
    }
    fn drive(&mut self)->Result<()>{
        let count=self.processed.len();
        for seq in 0..count {
            self.current_seq=seq;
            if self.processed[seq]{continue;}
            self.advance()?;
            let row=&self.recording.payload["actions"][seq];
            let a=&row["action"];
            let actor=a["player"].as_u64().unwrap() as usize;
            let kind=a["kind"].as_str().unwrap();
            if kind=="Move"&&a["sync_kind"]=="map_travel" {
                let coord=(a["coord"][0].as_i64(),a["coord"][1].as_i64());
                let actual=self.run.world.run.coord.map(|(x,y)|(Some(x as i64),Some(y as i64)));
                if actual!=Some(coord){return Err(fail("AUTOMATIC_MAP_TRAVEL_DIVERGENCE"));}
                // Original MoveToMapCoordAction is queued by the last vote.
                // Its own return can precede combat setup finishing, whereas
                // the vote's awaited return and next player input are settled.
                // It realizes no additional native decision: the preceding
                // MpMove and following before-checkpoint verify this fold.
                self.processed[seq]=true;self.finished[seq]=true;continue;
            }
            if kind=="SkipPotion" {self.finish(seq)?;continue;}
            if kind=="Choose" {
                let parent=row["parent_seq"].as_u64().ok_or_else(||fail("SELECTOR_WITHOUT_PARENT"))? as usize;
                let p=&self.recording.payload["actions"][parent];
                if p["action"]["card"]["id"]=="HEADBUTT"&&
                    self.run.combat().is_some_and(|c|c.pending_choice_for_player(actor as u8).is_none()) {
                    let offered=row["before"]["players"][actor]["discard"].as_array()
                        .ok_or_else(||fail("MISSING_SELECTOR_OFFER"))?;
                    if p["before"]["players"][actor]["discard"]!=Value::Array(offered.clone()) {
                        return Err(fail("FOLD_ORDER_DEPENDENT_SELECTOR"));
                    }
                    let offered=offered.iter().map(identity).collect::<Result<Vec<_>>>()?;
                    let selected=a["cards"].as_array().ok_or_else(||fail("MISSING_SELECTOR_RESULT"))?
                        .iter().map(identity).collect::<Result<Vec<_>>>()?;
                    folding::fold_forced_headbutt(&offered,&selected).map_err(from_fold)?;
                    self.finish(seq)?;continue;
                }
            }
            self.before(seq,actor)?;
            let command=match kind {
                "EventChoice"=>{
                    if self.run.phase!=MpPhase::Neow{return Err(fail("FOLD_EVENT_UNSUPPORTED"));}
                    let index=index(&a["index"])?;
                    let relic=self.run.neow[actor].get(index as usize).ok_or_else(||fail("INVALID_EVENT_OPTION"))?;
                    if a["option_id"]!=format!("NEOW.pages.INITIAL.options.{}",relic.name()){
                        return Err(fail("EVENT_OPTION_IDENTITY_MISMATCH"));
                    }
                    Command::MpChooseNeow{option:index}
                }
                "PickRelicReward"=>{
                    if self.run.relic_rewards[actor].map(|r|json!(r.name()))!=Some(a["relic_id"].clone()){
                        return Err(fail("RELIC_REWARD_IDENTITY_MISMATCH"));
                    }
                    Command::MpRelicReward{accept:true}
                }
                "OpenReward"=>{
                    let children:Vec<_>=self.children[seq].iter().copied().filter(|&i|
                        self.recording.payload["actions"][i]["action"]["kind"]=="PickCard").collect();
                    if children.len()!=1{return Err(fail("FOLD_MISSING_REWARD_CHOICE"));}
                    let child=&self.recording.payload["actions"][children[0]];
                    let c=&child["action"];
                    let offered=c["offered_cards"].as_array().ok_or_else(||fail("MISSING_CARD_OFFER"))?;
                    let actual=self.run.rewards.as_ref().ok_or_else(||fail("MISSING_NATIVE_REWARDS"))?[actor]
                        .cards.iter().map(checkpoint::card).collect::<std::result::Result<Vec<_>,_>>()?;
                    if actual!=*offered||c["reward_index"]!=a["reward_index"]{
                        return Err(fail("CARD_REWARD_OFFER_MISMATCH"));
                    }
                    let choice=CardRewardChoice{seq:children[0] as u32,parent_seq:seq as u32,
                        seat:index(&c["player"])? as u8,index:index(&c["index"])?,
                        offered:offered.iter().map(identity).collect::<Result<_>>()?,chosen:identity(&c["card"])?};
                    let fold=folding::fold_card_reward(seq as u32,actor as u8,Some(&choice)).map_err(from_fold)?;
                    self.processed[children[0]]=true;self.finished[children[0]]=true;
                    fold.command
                }
                "Collect" if a["reward_type"]=="gold"=>{
                    let r=&self.run.rewards.as_ref().ok_or_else(||fail("MISSING_NATIVE_REWARDS"))?[actor];
                    if r.gold.len()!=1{return Err(fail("FOLD_GOLD_SEQUENCE_UNSUPPORTED"));}
                    folding::fold_gold_tail(actor as u8,&r.gold,&[folding::GoldCollection{
                        seq:seq as u32,seat:actor as u8,amount:r.gold[0]}],Some(seq as u32)).map_err(from_fold)?.command
                }
                "PlayCard"=>{
                    let hand=index(&a["hand_index"])?;
                    let c=self.run.combat().ok_or_else(||fail("MISSING_NATIVE_COMBAT"))?;
                    let k=*c.players[actor as u8].hand.get(hand as usize).ok_or_else(||fail("INVALID_HAND_INDEX"))?;
                    if checkpoint::card(&c.cards[k as usize])?!=a["card"]{return Err(fail("CARD_IDENTITY_MISMATCH"));}
                    let target=if a["target"].is_null(){None}else{
                        Some(u16::try_from(index(&a["target"]["index"])?).map_err(|_|fail("INVALID_TARGET_INDEX"))?)
                    };
                    Command::MpPlayCard{hand,target}
                }
                "EndTurn"=>Command::MpEndTurn,
                "Choose"=>{
                    let indices=a["indices"].as_array().ok_or_else(||fail("MISSING_SELECTOR_INDICES"))?;
                    if indices.len()>1{return Err(fail("FOLD_MULTI_SELECTOR_UNSUPPORTED"));}
                    Command::MpResolveCombatChoice{index:indices.first().map(index).transpose()?}
                }
                "Move" if a["sync_kind"]=="map_vote"=>Command::MpMove{
                    x:i32::try_from(a["coord"][0].as_i64().ok_or_else(||fail("INVALID_MAP_COORD"))?)
                        .map_err(|_|fail("INVALID_MAP_COORD"))?,
                    y:i32::try_from(a["coord"][1].as_i64().ok_or_else(||fail("INVALID_MAP_COORD"))?)
                        .map_err(|_|fail("INVALID_MAP_COORD"))?,
                },
                _=>return Err(fail("ORIGINAL_INPUT_FOLD_UNSUPPORTED")),
            };
            self.execute(seq,actor,command,false)?;
            if command==Command::MpEndTurn {
                for seat in 0..self.recording.party.len() {
                    if self.run.combat().is_some_and(|c|c.needs_enemy_turn_acknowledgement(seat as u8)) {
                        self.execute(seq,seat,Command::MpReadyForEnemyTurn,true)?;
                    }
                }
            }
            self.finish(seq)?;
        }
        self.inputs_done=true;
        if self.live {return Ok(());}
        if self.finished.iter().any(|done|!*done){return Err(fail("UNCONSUMED_ORIGINAL_INPUT"));}
        if self.floors!=self.recording.payload["floor_digests"].as_array().unwrap().len() {
            return Err(fail("UNCONSUMED_FLOOR_CERTIFICATE"));
        }
        if self.run.phase!=MpPhase::RunOver||self.recording.payload["result"]["won"]!=false {
            return Err(fail("NATIVE_TERMINAL_DIVERGENCE"));
        }
        let state=checkpoint::project_state(&view(&self.run,&self.closed,self.local)?,
            &self.ledger.values,self.local)?;
        checkpoint::compare(&state,&self.recording.payload["result"]["terminal_state"])?;
        self.boundaries+=1;Ok(())
    }
}
/// Drives every original input; the replay (when one started) keeps the native plan
/// reproduced before the first divergence.
fn drive_all(recording:&Recording)->(Option<Replay<'_>>,Result<()>) {
    if crate::recording::validate_parents(&recording.payload).is_err() {
        return (None,Err(fail("INVALID_PARENT_SEQUENCE")));
    }
    if let Err(e)=crate::state_hash::validate(recording){return (None,Err(e.into()));}
    let mut replay=match Replay::new(recording){Ok(replay)=>replay,Err(e)=>return (None,Err(e))};
    let initialized=replay.run.initialization_checkpoint_view(replay.local)
        .map_err(|_|fail("INITIALIZATION_VIEW_UNAVAILABLE"))
        .and_then(|view|crate::certification::initialization(recording,&view).map_err(Into::into));
    if let Err(e)=initialized {return (None,Err(e));}
    let result=replay.drive().map_err(|mut e|{if e.seq.is_none(){e.seq=Some(replay.current_seq);}e});
    (Some(replay),result)
}
pub fn verify(recording:&Recording)->Result<VerifiedParty<'_>> {
    require_training_ascension(recording)?;
    let (replay,result)=drive_all(recording);
    result?;
    let replay=replay.expect("a fully driven recording has a replay");
    Ok(VerifiedParty{recording,steps:replay.steps,ops:replay.ops,boundaries:replay.boundaries,floors:replay.floors})
}
pub(crate) fn reproduce(recording:&Recording)->crate::Reproduction {
    let (replay,result)=drive_all(recording);
    let (steps,ops,inputs_done)=replay.map_or((Vec::new(),Vec::new(),false),|r|(r.steps,r.ops,r.inputs_done));
    crate::Reproduction{steps,ops,inputs_done,failure:result.err()}
}

fn live_replay(recording:&Recording,encoder:NativeEncoder)->Result<Replay<'_>>{
    let mut replay=Replay::new(recording)?;
    crate::certification::initialization(recording,&replay.run.initialization_checkpoint_view(replay.local)
        .map_err(|_|fail("INITIALIZATION_VIEW_UNAVAILABLE"))?)?;
    replay.live=true;
    replay.encoder=encoder;
    if let Err(mut error)=replay.drive() {
        // Opening a multiplayer reward precedes its nested card confirmation.
        // No native action has been dispatched by this branch yet.
        if error.code!="FOLD_MISSING_REWARD_CHOICE" {
            if error.seq.is_none(){error.seq=Some(replay.current_seq);}
            return Err(error);
        }
    }
    Ok(replay)
}

pub(crate) fn live_probe_steps(recording:&Recording,probes:&[Recording],
    encoder:impl Fn()->NativeEncoder)->Result<Vec<Vec<ReplayStep>>>{
    let base=live_replay(recording,encoder())?;
    let count=base.steps.len();
    probes.iter().map(|recording|{
        let rows=recording.payload["actions"].as_array().unwrap();
        let mut children=vec![Vec::new();rows.len()];
        for (i,row) in rows.iter().enumerate(){
            if let Some(parent)=row["parent_seq"].as_u64(){children[parent as usize].push(i);}
        }
        let mut processed=base.processed.clone();processed.resize(rows.len(),false);
        let mut finished=base.finished.clone();finished.resize(rows.len(),false);
        let mut replay=Replay{live:true,recording,run:base.run.clone(),encoder:encoder(),
            obs:Box::new(NativeObsV1::new()),decision:base.decision,closed:base.closed.clone(),
            ledger:base.ledger.clone(),steps:base.steps.clone(),ops:base.ops.clone(),processed,finished,children,
            boundaries:base.boundaries,local:base.local,current_seq:base.current_seq,floors:base.floors,inputs_done:false};
        replay.drive().map_err(|mut e|{if e.seq.is_none(){e.seq=Some(replay.current_seq);}e})?;
        Ok(replay.steps.into_iter().skip(count).collect())
    }).collect()
}

pub(crate) fn live_pending(recording:&Recording,encoder:NativeEncoder)->Result<(Vec<crate::live::Pending>,crate::live::LiveRun)>{
    let mut replay=live_replay(recording,encoder)?;
    replay.advance()?;
    let mut pending=Vec::new();
    for actor in 0..recording.party.len() {
        let decision=DecisionId{actor:actor as u8,..replay.decision};
        let mut obs=Box::new(NativeObsV1::new());
        replay.encoder.encode_multiplayer(&replay.run,decision,actor as u64+1,&mut obs)
            .map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        pending.push(crate::live::Pending::new(obs,decision,actor as u64+1)?);
    }
    Ok((pending,crate::live::LiveRun::Party(replay.run)))
}
