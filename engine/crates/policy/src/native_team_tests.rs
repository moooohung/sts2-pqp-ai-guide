use super::*;

fn joint() -> NativeDecision {
    let mut obs = NativeObsV1::new();
    let decision = DecisionId::default();
    obs.clear(decision);
    obs.push_row(Row { kind:RowKind::Global,..Row::default() }).unwrap();
    let mut seats = Vec::new();
    for owner in 0..2 {
        let mut row=Row {kind:RowKind::Seat,..Row::default()};
        row.numbers.set_integer(0,owner).unwrap();
        row.numbers.set_integer(5,owner+1).unwrap();
        seats.push(obs.push_row(row).unwrap());
    }
    for owner in 0..2 {
        let source=obs.push_row(Row {kind:RowKind::Card,owner:seats[owner],..Row::default()}).unwrap();
        let command=Command::MpPlayCard {hand:owner as u32,target:None};
        obs.push_candidate(Candidate {action:ActionKind::PlayCard,source,present:true,legal:true,
            ..Candidate::default()},Binding {decision,command,source_instance:Some(owner as u32),
                ..Binding::default()}).unwrap();
    }
    obs.finish(DecisionStatus::Actionable).unwrap();
    let mut view=obs.view().unwrap();
    let actors=[0,1];
    view.candidates=CandidatesView {actor:&actors,..view.candidates};
    view.scope=DecisionScope::TeamCombat;
    view.identity=TEAM_MANIFEST_IDENTITY;
    NativeDecision::from_view(view).unwrap()
}

#[test]
fn joint_candidates_route_to_their_own_resources_in_both_cpu_and_packed_scorers() {
    let input=joint();
    assert_eq!(input.action_actor(0),Some(0));
    assert_eq!(input.action_actor(1),Some(1));
    assert_eq!(input.action_slot(0),Some(0));
    assert_eq!(input.action_slot(1),Some(1));
    let _rng_guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();
    let policy=NativePolicy::init_joint(8,&device);
    let packed=policy.forward(&[&input],&device);
    let expected=packed.logp.into_data().try_to_vec::<f32>().unwrap();
    let value=packed.value.into_data().try_to_vec::<f32>().unwrap()[0];
    let cpu=CpuPolicy::copy_from(&policy);
    let mut workspace=CpuWorkspace::default();
    let actual=cpu.forward(&input,&mut workspace);
    assert!((actual-value).abs()<1e-5);
    for (a,b) in workspace.logp.iter().zip(&expected) {assert!((a-b).abs()<1e-5,"{a} vs {b}");}
    assert!((workspace.logp.iter().map(|p|p.exp()).sum::<f32>()-1.).abs()<1e-5);
    assert!(workspace.logp.iter().all(|p|p.is_finite()));
}
