//! Original-input v2 native replay and training samples.
//!
//! Bounded loading, explicit folding and checkpoint certification. Unsupported
//! or divergent runs are excluded; only VerifiedRun yields rollout samples.
//! Commands are the actual
//! NativeObsV1 binding types; no action-space/observation encoder is duplicated.
#![forbid(unsafe_code)]
use sts2_native_obs::*;
pub mod folding;
pub mod recording;
pub mod checkpoint;
pub mod state_hash;
pub mod party;
pub mod certification;
pub mod solo;
pub mod view;
pub mod live;

/// The native plan an original-input recording reproduces, up to its first divergence.
/// `inputs_done` means every original input was consumed; a failure after that is only an
/// end-of-run certificate (e.g. a victory, which `verify` does not admit to training).
pub struct Reproduction {
    pub steps:Vec<party::ReplayStep>,
    pub(crate) ops:Vec<party::Op>,
    pub inputs_done:bool,
    pub failure:Option<party::Failure>,
}
pub fn reproduce(recording:&recording::Recording)->Reproduction{
    if recording.party.len()>1 {party::reproduce(recording)} else {solo::reproduce(recording)}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    InvalidRecording,
    ObservationUnavailable,
    NotActionable,
    SeatMismatch,
    LegalSetMismatch,
    ChosenActionNotLegal,
    AmbiguousIdentity,
}

/// Identity sidecar, not learned inputs. Event commands retain native
/// EventOptionId rather than translated display text or an inferred index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionIdentity {
    pub command: Command,
    pub source_instance: Option<u32>,
    pub target_instance: Option<u32>,
    pub offer_instance: Option<u64>,
}
impl From<Binding> for ActionIdentity {
    fn from(b: Binding) -> Self {
        Self { command: b.command, source_instance:b.source_instance,
            target_instance:b.target_instance, offer_instance:b.offer_instance }
    }
}

/// Optional typed-frame verification primitive, not the v2 wire schema.
/// V2 retains original inputs; native folding uses the simulator's legal set
/// at the corresponding native decision, without changing its action space.
pub struct DecisionFrame {
    pub seat:u8,
    pub legal:Vec<ActionIdentity>,
    pub chosen:ActionIdentity,
}

/// Borrows the original NativeObsV1 directly, with the exact candidate index
/// and mask; no converter or copied observation layout. Reward/terminal are
/// intentionally absent until independently replayed checkpoint semantics
/// exist (never copy an unverified recording's claimed reward into training).
pub struct CheckedDecision<'a> {
    pub observation:ObservationView<'a>,
    pub legal_mask:Vec<bool>,
    pub chosen_action_index:usize,
    pub seat:u8,
}

pub fn check_decision<'a>(obs:&'a NativeObsV1,current:DecisionId,net_id:u64,
    recorded:&DecisionFrame)->Result<CheckedDecision<'a>,Reason>
{
    let view=obs.view().map_err(|_|Reason::ObservationUnavailable)?;
    if view.status!=DecisionStatus::Actionable{return Err(Reason::NotActionable)}
    if view.decision!=current||recorded.seat!=current.actor{return Err(Reason::SeatMismatch)}
    let mut legal=Vec::new();
    let mut mask=Vec::with_capacity(view.candidate_len);
    let mut selected=None;
    for index in 0..view.candidate_len {
        let allowed=view.candidates.sampleable(index,view.candidate_len);
        mask.push(allowed);
        if !allowed{continue}
        let binding=obs.binding(index,current,net_id).map_err(|_|Reason::SeatMismatch)?;
        let identity=ActionIdentity::from(binding);
        if legal.contains(&identity){return Err(Reason::AmbiguousIdentity)}
        legal.push(identity);
        if identity==recorded.chosen {selected=Some(index);}
    }
    // Preserve actual offered order as part of the contract. This includes
    // legal options not chosen by the player; chosen-only checking is unsafe.
    if legal!=recorded.legal{return Err(Reason::LegalSetMismatch)}
    Ok(CheckedDecision{chosen_action_index:selected.ok_or(Reason::ChosenActionNotLegal)?,
        legal_mask:mask,seat:current.actor,observation:view})
}

/// Parse without certification. V1 is retained only as quarantined raw data;
/// it has no usable numeric initialization and is never silently upgraded.
pub fn load_recording(bytes:&[u8])->Result<recording::Recording,Reason>{
    recording::load(bytes)
}

/// Fully checked episodes only; both variants stream the trainer's native
/// observation view without a converter or a dependency on trainer internals.
pub enum VerifiedRun<'a>{Solo(solo::VerifiedSolo<'a>),Party(party::VerifiedParty<'a>)}
impl VerifiedRun<'_>{
    pub fn steps(&self)->&[party::ReplayStep]{match self{Self::Solo(v)=>v.steps(),Self::Party(v)=>v.steps()}}
    pub fn checked_boundaries(&self)->usize{match self{Self::Solo(v)=>v.checked_boundaries(),Self::Party(v)=>v.checked_boundaries()}}
    pub fn checked_floors(&self)->usize{match self{Self::Solo(v)=>v.checked_floors(),Self::Party(v)=>v.checked_floors()}}
    pub fn rollout(&self,reward:&sts2_native_obs::reward::Reward,emit:impl FnMut(party::RolloutSample<'_>))
        ->Result<(),party::Failure>{
        match self{Self::Solo(v)=>v.rollout(reward,emit),Self::Party(v)=>v.rollout(reward,emit)}
    }
}
pub fn verify(recording:&recording::Recording)->Result<VerifiedRun<'_>,party::Failure>{
    if recording.party.len()==1 {solo::verify(recording).map(VerifiedRun::Solo)}
    else{party::verify(recording).map(VerifiedRun::Party)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_sim::run::{RunManager,RunOptions,mp::{Character,MpRun,PlayerSpec}};
    fn frame(obs:&NativeObsV1,id:DecisionId,net:u64)->DecisionFrame{
        let view=obs.view().unwrap();
        let legal:Vec<_>=(0..view.candidate_len)
            .filter(|&i|view.candidates.sampleable(i,view.candidate_len))
            .map(|i|obs.binding(i,id,net).unwrap().into()).collect();
        DecisionFrame{seat:id.actor,chosen:legal[0],legal}
    }
    #[test]
    fn solo_native_identity_and_legal_mask_for_five_characters(){
        for c in [Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder]{
            let run=RunManager::new_for_character(1,RunOptions::default(),c);
            let mut encoder=NativeEncoder::new();let mut obs=NativeObsV1::new();
            let id=DecisionId{episode:1,generation:0,actor:0};
            encoder.encode_solo(&run,id,&mut obs).unwrap();
            let mut f=frame(&obs,id,1);
            let checked=check_decision(&obs,id,1,&f).unwrap();
            assert!(checked.legal_mask[checked.chosen_action_index]);
            f.legal.pop();assert!(matches!(check_decision(&obs,id,1,&f),Err(Reason::LegalSetMismatch)));
        }
    }
    #[test]
    fn all_multiplayer_seats_validate_and_wrong_owner_is_rejected(){
        for count in 2..=4{
            let specs:Vec<_>=(0..count).map(|i|PlayerSpec{net_id:i as u64+1,character:Character::Ironclad}).collect();
            let run=MpRun::new("HUMAN-REPLAY-CONTRACT",&specs,10).unwrap();
            let mut encoder=NativeEncoder::new();let mut obs=NativeObsV1::new();
            for seat in 0..count{
                let id=DecisionId{episode:1,generation:0,actor:seat as u8};
                encoder.encode_multiplayer(&run,id,specs[seat].net_id,&mut obs).unwrap();
                let f=frame(&obs,id,specs[seat].net_id);
                assert!(check_decision(&obs,id,specs[seat].net_id,&f).is_ok());
                assert!(matches!(check_decision(&obs,id,999,&f),Err(Reason::SeatMismatch)));
            }
        }
    }
    #[test]
    fn wire_input_never_silently_promotes(){
        for bytes in [b"{}".as_slice(),b"{\"schema\":\"sts2-human-run-v1\"}",b"{\"schema\":\"sts2-human-run-v2\"}"]{
            assert!(matches!(load_recording(bytes),Err(Reason::InvalidRecording)));
        }
    }
}
