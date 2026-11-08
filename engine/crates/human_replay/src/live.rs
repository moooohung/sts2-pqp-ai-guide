//! Live prefixes use the original replay folders, never a second action translator.
//! Prefix reconstruction also handles parents whose nested choices arrive later.
use serde_json::Value;
use sts2_native_obs::*;
use crate::{party::{Failure,fail},recording::Recording};

pub struct Pending {
    pub observation:Box<NativeObsV1>,
    pub decision:DecisionId,
    pub legal:Vec<(usize,Binding)>,
}
impl Pending {
    pub(crate) fn new(observation:Box<NativeObsV1>,decision:DecisionId,net:u64)->Result<Self,Failure>{
        let view=observation.view().map_err(|_|fail("NATIVE_OBSERVATION_FAILED"))?;
        let mut legal=Vec::new();
        if view.status==DecisionStatus::GameplayFault {return Err(fail("NATIVE_GAMEPLAY_FAULT"));}
        if view.status==DecisionStatus::Actionable {
            for slot in 0..view.candidate_len {
                if view.candidates.sampleable(slot,view.candidate_len) {
                    legal.push((slot,observation.binding(slot,decision,net)
                        .map_err(|_|fail("NATIVE_BINDING_FAILED"))?));
                }
            }
        }
        Ok(Self{observation,decision,legal})
    }
}

/// Worker-owned original-input journal. No final result or floor digests required.
/// `pending` reconstructs the prefix through the shared replay implementation;
/// a refused prefix never yields observations from the last successful state.
pub struct LiveMirror {recording:Recording}
impl LiveMirror {
    pub fn new(run_start:&[u8])->Result<Self,Failure>{
        let recording=crate::recording::load_run_start(run_start)
            .map_err(|_|fail("INVALID_RUN_START"))?;
        Ok(Self{recording})
    }
    pub fn consume(&mut self,row:Value)->Result<(),Failure>{
        let rows=self.recording.payload["actions"].as_array_mut().unwrap();
        let seq=rows.len();
        if seq>=100_000 || row["seq"].as_u64()!=Some(seq as u64)
            || row["action"]["player"].as_u64().is_none_or(|s|s>=self.recording.party.len() as u64)
            || row["action"]["kind"].as_str().is_none()
            || !row["before"].is_object()
            || (!row["parent_seq"].is_null() && row["parent_seq"].as_u64().is_none_or(|s|s>=seq as u64)) {
            return Err(Failure{code:"INVALID_ORIGINAL_INPUT",seq:Some(seq),path:String::new()});
        }
        rows.push(row);
        Ok(())
    }
    pub fn pending(&self,encoder:NativeEncoder)->Result<Vec<Pending>,Failure>{
        if self.recording.party.len()==1 {crate::solo::live_pending(&self.recording,encoder)}
        else {crate::party::live_pending(&self.recording,encoder)}
    }

    /// Resolve an independently captured live action through the very same
    /// original-input folder as an actual confirmation. Never mutate the mirror.
    pub fn resolve(&self,action:Value,before:Value,parent:Option<usize>,
        encoder:impl Fn()->NativeEncoder)->Result<crate::party::ReplayStep,Failure>{
        self.resolve_many(&[action],before,parent,encoder)?.into_iter().next()
            .ok_or_else(||fail("LIVE_OPTION_HAS_NO_NATIVE_ACTION"))
    }

    pub fn resolve_many(&self,actions:&[Value],before:Value,parent:Option<usize>,
        encoder:impl Fn()->NativeEncoder)->Result<Vec<crate::party::ReplayStep>,Failure>{
        let mut probes=Vec::with_capacity(actions.len());
        for action in actions {
            let mut probe=Self{recording:self.recording.clone()};
            let seq=probe.recording.payload["actions"].as_array().unwrap().len();
            probe.consume(serde_json::json!({"seq":seq,"action":action,"before":before,"parent_seq":parent}))?;
            probes.push(probe.recording);
        }
        let results=if self.recording.party.len()==1 {crate::solo::live_probe_steps(&self.recording,&probes,encoder)?}
            else {crate::party::live_probe_steps(&self.recording,&probes,encoder)?};
        results.into_iter().map(|steps|{
            let mut added=steps.into_iter().filter(|s|!s.forced);
            let chosen=added.next().ok_or_else(||fail("LIVE_OPTION_HAS_NO_NATIVE_ACTION"))?;
            if added.next().is_some(){return Err(fail("LIVE_OPTION_HAS_MULTIPLE_NATIVE_ACTIONS"));}
            Ok(chosen)
        }).collect()
    }
}

