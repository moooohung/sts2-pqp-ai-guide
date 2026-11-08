//! Dashboard replay of an original-input recording: the reproduced native plan
//! re-simulated through the shared replay projection AI play uses, so a human run
//! draws in the same theater. Only reproduced decisions become frames; a run
//! that diverges stops at the divergence and says where.
use serde_json::{Map,Value,json};
use sts2_native_obs::*;
use sts2_native_obs::replay::{Game,RealSource};
use sts2_native_obs::reward::{party_objective_complete,solo_objective_complete};
use sts2_sim::run::{RunPhase,mp::MpPhase};
use crate::{party::{Failure,Op},recording::Recording,Reproduction};

pub const VIEW_SCHEMA:&str="sts2-human-replay-view-v1";

struct Frames {frames:Vec<Value>,maps:Map<String,Value>,acts:Vec<u64>}
impl Frames {
    fn push(&mut self,source:&RealSource<'_>,slot:usize)->Result<(),String>{
        let act=source.game.act();
        if !self.acts.contains(&act){self.acts.push(act);self.maps.insert(act.to_string(),source.replay_map());}
        let frame=source.replay_frame(slot as i32,self.frames.len())?;
        self.frames.push(frame);Ok(())
    }
}
fn source<'a>(game:Game<'a>,obs:&'a NativeObsV1,decision:DecisionId,seats:usize)->RealSource<'a>{
    let (inventory,floor)=(game.inventory(),game.floor());
    RealSource{game,obs,decision,team:None,seats,inventory,floor}
}
/// The end row AI play writes when a game is over: seat inventories, floor, length and result.
fn end(game:Game<'_>,won:bool,len:usize)->Value{
    let mut end=game.inventory();
    end["type"]=json!("end");end["floor"]=json!(game.floor());end["len"]=json!(len);
    end["end"]=json!(if won {"win"} else {"death"});
    end
}
fn solo(recording:&Recording,plan:&Reproduction,out:&mut Frames)->Result<Option<Value>,String>{
    let mut game=crate::solo::fresh(recording).map_err(|e|e.code.to_string())?;
    let mut encoder=NativeEncoder::new();let mut obs=Box::new(NativeObsV1::new());
    let mut decision=DecisionId{episode:recording.numeric_seed.unwrap(),generation:0,actor:0};
    for s in &plan.steps {
        encoder.encode_solo(&game,decision,&mut obs).map_err(|e|format!("{e:?}"))?;
        out.push(&source(Game::Solo(&game),&obs,decision,1),s.candidate_slot)?;
        encoder.dispatch_solo(&mut game,&mut decision,&mut obs,s.candidate_slot).map_err(|e|format!("{e:?}"))?;
    }
    let won=solo_objective_complete(&game);
    Ok((won||game.phase()==RunPhase::RunOver).then(||end(Game::Solo(&game),won,out.frames.len())))
}
fn party(recording:&Recording,plan:&Reproduction,out:&mut Frames)->Result<Option<Value>,String>{
    let mut run=crate::party::fresh(recording).map_err(|e|e.code.to_string())?;
    let mut encoder=NativeEncoder::new();let mut obs=Box::new(NativeObsV1::new());
    let mut decision=DecisionId{episode:recording.numeric_seed.unwrap(),generation:0,actor:0};
    let seats=recording.party.len();
    for op in &plan.ops {
        match *op {
            Op::Advance=>{NativeEncoder::advance_multiplayer(&mut run,&mut decision).map_err(|e|format!("{e:?}"))?;}
            Op::Action(i)=>{
                let s=&plan.steps[i];decision.actor=s.actor;
                let net=s.actor as u64+1;
                encoder.encode_multiplayer(&run,decision,net,&mut obs).map_err(|e|format!("{e:?}"))?;
                out.push(&source(Game::Party(&run),&obs,decision,seats),s.candidate_slot)?;
                encoder.dispatch_multiplayer(&mut run,&mut decision,net,&mut obs,s.candidate_slot)
                    .map_err(|e|format!("{e:?}"))?;
            }
        }
    }
    let won=party_objective_complete(&run);
    Ok((won||run.phase==MpPhase::RunOver).then(||end(Game::Party(&run),won,out.frames.len())))
}
fn failure(f:&Failure)->Value{json!({"code":f.code,"seq":f.seq,"path":f.path})}

/// `{schema,header,maps,frames,end,complete,failure}` for a gzipped or plain v2 recording.
/// `complete` means every original input was reproduced; otherwise `failure` names the
/// first input that could not be, and the frames stop just before it.
pub fn replay(bytes:&[u8])->Value{
    let Ok(recording)=crate::load_recording(bytes) else {
        return json!({"schema":VIEW_SCHEMA,"frames":[],"maps":{},"end":null,"complete":false,
            "failure":{"code":"INVALID_RECORDING","seq":null,"path":""}});
    };
    let p=&recording.payload;
    let characters:Vec<Value>=p["party"].as_array().map(|seats|seats.iter().map(|s|s["character"].clone()).collect()).unwrap_or_default();
    let names:Vec<&str>=characters.iter().filter_map(Value::as_str).collect();
    let header=json!({"type":"header","v":2,"source":"human","reason":"human","seed":p["seed"],
        "character":names.join("+"),"characters":characters,"party_size":characters.len(),"ascension":p["ascension"],
        "local_seat":p["local_seat"],"game_version":p["game_version"],"recording_schema":p["schema"]});
    let plan=crate::reproduce(&recording);
    let mut out=Frames{frames:Vec::new(),maps:Map::new(),acts:Vec::new()};
    let mut failed=plan.failure.as_ref().map(failure);
    let mut end=Value::Null;
    if recording.is_v2() && !recording.party.is_empty() {
        let built=if recording.party.len()>1 {party(&recording,&plan,&mut out)} else {solo(&recording,&plan,&mut out)};
        match built {
            Ok(Some(row)) if plan.inputs_done=>end=row,
            Ok(_)=>{},
            Err(error)=>failed=Some(json!({"code":"VIEW_RESIMULATION_FAILED","seq":null,"path":error})),
        }
    }
    let complete=plan.inputs_done && !end.is_null();
    json!({"schema":VIEW_SCHEMA,"header":header,"maps":out.maps,"frames":out.frames,"end":end,
        "complete":complete,"failure":if complete {Value::Null} else {failed.unwrap_or(Value::Null)}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unreadable_and_v1_recordings_give_no_frames_and_name_the_reason(){
        let bad=replay(b"{}");
        assert_eq!(bad["failure"]["code"],"INVALID_RECORDING");
        assert_eq!(bad["frames"].as_array().unwrap().len(),0);
        assert_eq!(bad["complete"],false);
    }
}
