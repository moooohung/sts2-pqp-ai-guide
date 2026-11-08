//! Initialization and profile inputs are independent replay evidence, not
//! learner features. Native partial unlock profiles are deliberately excluded.
use serde_json::{Value,json};
use sts2_sim::run::{mp::MpUnlockState,replay_view::ReplayCheckpointView};
use crate::{checkpoint::{self,Mismatch},recording::Recording};

pub fn initialization(recording:&Recording,view:&ReplayCheckpointView<'_>)
    ->Result<(),Mismatch> {
    checkpoint::compare(&checkpoint::project_state(view,&[],0)?,
        &recording.payload["initial_state"])?;
    // Native ActDef.name is a C# type name; the recording stores ModelId.Entry.
    let acts=view.act_order.iter().map(|s|s.to_ascii_uppercase()).collect::<Vec<_>>();
    checkpoint::compare(&json!(acts),&recording.payload["setup"]["acts"])?;
    let profiles=recording.payload["setup"]["unlocks"].as_array()
        .ok_or_else(||Mismatch::unsupported("initial unlock profiles"))?;
    if profiles.len()!=view.seats.len() {
        return Err(Mismatch::unsupported("initial unlock profile count"));
    }
    let native=MpUnlockState::all();
    let mut epochs=native.unlocked_epochs.iter().map(|s|s.to_string()).collect::<Vec<_>>();
    epochs.sort();
    let mut encounters=native.encounters_seen.iter().map(|s|format!("ENCOUNTER.{s}")).collect::<Vec<_>>();
    encounters.sort();
    for (seat,p) in profiles.iter().enumerate() {
        let mut p=p.clone();
        // Epochs/seen encounters are sets in the C# source. Preserve duplicates
        // so malformed duplicate entries cannot silently normalize away.
        for key in ["epochs","encounters"] {
            let a=p[key].as_array_mut().ok_or_else(||Mismatch::unsupported(key))?;
            a.sort_by(|a,b|a.as_str().cmp(&b.as_str()));
        }
        let expected:Value=json!({"seat":seat,"epochs":epochs,"encounters":encounters,
            "number_of_runs":native.number_of_runs});
        checkpoint::compare(&expected,&p)?;
    }
    Ok(())
}
