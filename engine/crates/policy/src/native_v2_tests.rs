use super::*;

pub(super) fn observation() -> Box<NativeObsV1> {
    let mut obs=Box::new(NativeObsV1::new());
    let decision=DecisionId::default();
    obs.clear(decision);
    obs.push_row(Row {kind:RowKind::Global,..Row::default()}).unwrap();
    let mut seat=Row {kind:RowKind::Seat,..Row::default()};
    for (lane,value) in [(0,0),(2,40),(3,80),(5,2),(6,3)] {seat.numbers.set_integer(lane,value).unwrap();}
    let seat=obs.push_row(seat).unwrap();
    let first=obs.push_row(Row {kind:RowKind::Creature,..Row::default()}).unwrap();
    let mut enemy=Row {kind:RowKind::Creature,..Row::default()};
    enemy.numbers.set_integer(0,30).unwrap();
    let second=obs.push_row(enemy).unwrap();
    let mut intent=Row {kind:RowKind::Intent,parent:first,owner:first,..Row::default()};
    intent.numbers.set_integer(0,17).unwrap();
    let intent=obs.push_row(intent).unwrap();
    obs.push_edge(Edge {from:intent,to:first,relation:Relation::Parent}).unwrap();
    let mut power=Row {kind:RowKind::Power,parent:second,owner:second,..Row::default()};
    power.numbers.set_integer(0,5).unwrap();
    let power=obs.push_row(power).unwrap();
    obs.push_edge(Edge {from:power,to:first,relation:Relation::Target}).unwrap();
    let source=obs.push_row(Row {kind:RowKind::Card,owner:seat,parent:seat,
        detail:[ContentId::ABSENT,ContentId::ABSENT,ContentId::ABSENT,
            ContentId::from_index(Namespace::PileKind,sts2_sim::combat::PileKind::None as usize).unwrap()],
        ..Row::default()}).unwrap();
    let a=obs.push_row(Row {kind:RowKind::MapNode,..Row::default()}).unwrap();
    let mut map=Row {kind:RowKind::MapNode,..Row::default()};
    map.numbers.set_integer(1,4).unwrap();
    let b=obs.push_row(map).unwrap();
    obs.push_edge(Edge {from:a,to:b,relation:Relation::MapConnection}).unwrap();
    for (i,target) in [first,second].into_iter().enumerate() {
        let command=Command::MpPlayCard {hand:i as u32,target:None};
        obs.push_candidate(Candidate {action:command.action(),source,target,present:true,legal:true,
            model:ContentId::from_index(Namespace::CardId,i).unwrap(),
            ..Candidate::default()},Binding {decision,command,source_instance:Some(i as u32),
                ..Binding::default()}).unwrap();
    }
    obs.finish(DecisionStatus::Actionable).unwrap();
    obs
}

#[test]
fn packing_retains_typed_child_ownership_target_and_directed_map_edges_once() {
    let obs=observation();
    let d=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    for expected in [
        [5,3,Relation::Parent.wire_id() as usize],[5,3,Relation::Owner.wire_id() as usize],
        [6,4,Relation::Parent.wire_id() as usize],[6,4,Relation::Owner.wire_id() as usize],
        [6,3,Relation::Target.wire_id() as usize],[8,9,Relation::MapConnection.wire_id() as usize],
    ] {
        assert_eq!(d.relations.iter().filter(|&&r|r==expected).count(),1);
    }
    let round:NativeDecision=serde_json::from_slice(&serde_json::to_vec(&d).unwrap()).unwrap();
    assert_eq!(round.relations,d.relations);
    assert_eq!(round.sources,vec![7,7]);
    assert_eq!(round.targets,vec![3,4]);
    assert_eq!(round.slots,vec![0,1]);
}

#[test]
fn candidates_and_critic_keep_direct_seat_global_and_known_ratios() {
    let obs=observation();
    let d=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    assert_eq!(d.direct.global,1);
    assert_eq!(d.direct.value_seat,2);
    assert_eq!(d.direct.seats,vec![2,2]);
    assert_eq!(d.direct.ratios[2],[0.5,1.,2./3.,1.]);
    let device=Device::flex();
    let p=crate::native_batch::NativeHostBatch::pack(&[&d]).upload(&device);
    let ints=p.ints.clone().into_data().try_to_vec::<i32>().unwrap();
    assert_eq!(&ints[p.direct_i()..][..4],&[1,2,2,2]);
    let floats=p.floats.clone().into_data().try_to_vec::<f32>().unwrap();
    assert_eq!(&floats[p.ratio_f()+2*4..][..4],&[0.5,1.,2./3.,1.]);
}

#[test]
fn direct_hp_and_global_context_change_actor_and_critic_with_cpu_parity() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();device.seed(20261103);
    let mut model=NativePolicy::init_v2(8,&device);
    model.actor=LinearConfig::new(8,1).with_initializer(burn::nn::Initializer::Zeros).init(&device);
    model.critic=LinearConfig::new(8,1).with_initializer(burn::nn::Initializer::Zeros).init(&device);
    let obs=observation();
    let d=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    let before=model.forward(&[&d],&device);
    let lp=before.logp.into_data().try_to_vec::<f32>().unwrap();
    let value=before.value.into_data().try_to_vec::<f32>().unwrap()[0];
    for changed in [2,1] {
        let mut other=d.clone();
        other.rows[changed-1].numbers[2]=scaled_number(8.);
        other.rows[changed-1].ip|=1<<2;
        let after=model.forward(&[&other],&device);
        let actual=after.logp.into_data().try_to_vec::<f32>().unwrap();
        let expected_value=after.value.into_data().try_to_vec::<f32>().unwrap()[0];
        assert!(actual.iter().zip(&lp).any(|(a,b)|(a-b).abs()>1e-6));
        eprintln!("direct_counterfactual row={changed} max_abs_logp_delta={:e} abs_value_delta={:e}",
            actual.iter().zip(&lp).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max),(expected_value-value).abs());
        assert!((expected_value-value).abs()>1e-6);
        let mut w=CpuWorkspace::default();
        let cpu_value=CpuPolicy::copy_from(&model).forward(&other,&mut w);
        assert!((cpu_value-expected_value).abs()<2e-5);
        assert!(w.logp.iter().zip(actual).all(|(a,b)|(a-b).abs()<2e-5));
        assert_eq!(other.slots,d.slots);
    }
}

#[test]
fn independent_combat_deck_map_means_cannot_dilute_each_other() {
    let device=Device::flex();
    let obs=observation();
    let mut d=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    assert_eq!(d.groups,vec![0,0,1,1,1,1,2,3,3]);
    let model=NativePolicy::init_v2(1,&device);
    for extra in [0,19] {
        for _ in 0..extra {d.rows.push(d.rows[8].clone());d.groups.push(3);}
        let p=crate::native_batch::NativeHostBatch::pack(&[&d]).upload(&device);
        let mut state=vec![0.;p.r+1];
        for (i,&group) in d.groups.iter().enumerate() {state[i+1]=[0.,2.,5.,11.][group as usize];}
        let pools=model.v2.as_ref().unwrap().pools(&p,Tensor::from_data(TensorData::new(state,[1,p.r+1,1]),&device));
        let values=pools.into_data().try_to_vec::<f32>().unwrap();
        for (a,b) in values.iter().zip([2.,5.,11.]) {assert!((a-b).abs()<2e-6);}
    }
}

#[test]
fn owner_target_and_directed_map_counterfactuals_change_relational_inputs_and_preferences() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();device.seed(20261103);
    let mut model=NativePolicy::init_v2(8,&device);
    model.actor=LinearConfig::new(8,1).with_initializer(burn::nn::Initializer::Zeros).init(&device);
    let obs=observation();
    let original=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    for case in 0..3 {
        let mut before=original.clone();
        if case==2 {before.sources=vec![8,9];before.targets=vec![0,0];}
        let mut after=before.clone();
        match case {
            0=>for edge in &mut after.relations {
                if (edge[0]==5 || edge[0]==6) &&
                    [Relation::Parent.wire_id() as usize,Relation::Owner.wire_id() as usize].contains(&edge[2]) {
                    edge[1]=7-edge[1];
                }
            },
            1=>for edge in &mut after.relations {if edge[2]==Relation::Target.wire_id() as usize {edge[1]=4;}},
            _=>for edge in &mut after.relations {if edge[2]==Relation::MapConnection.wire_id() as usize {edge.swap(0,1);}},
        }
        let a=crate::native_batch::NativeHostBatch::pack(&[&before]).upload(&device);
        let b=crate::native_batch::NativeHostBatch::pack(&[&after]).upload(&device);
        assert_ne!(a.ints.clone().into_data(),b.ints.clone().into_data());
        let out_a=model.forward_packed(&a).logp.into_data().try_to_vec::<f32>().unwrap();
        let out_b=model.forward_packed(&b).logp.into_data().try_to_vec::<f32>().unwrap();
        assert!(out_a.iter().zip(&out_b).any(|(a,b)|(a-b).abs()>1e-6),"case {case}");
        eprintln!("relation_counterfactual case={case} max_abs_logp_delta={:e}",
            out_a.iter().zip(&out_b).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max));
        let mut w=CpuWorkspace::default();
        CpuPolicy::copy_from(&model).forward(&after,&mut w);
        assert!(w.logp.iter().zip(out_b).all(|(a,b)|(a-b).abs()<2e-5),"case {case}");
        assert_eq!(before.slots,after.slots);
    }
}

#[test]
fn relational_critic_is_nonlinear_and_refit_freezes_representation() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex().autodiff();
    let mut model=NativePolicy::init_v2(8,&device);
    let v2=model.v2.as_mut().unwrap();
    v2.critic_hidden=LinearConfig::new(44,8).with_initializer(burn::nn::Initializer::Zeros).init(&device);
    v2.critic=LinearConfig::new(8,1).with_initializer(burn::nn::Initializer::Zeros).init(&device);
    let mut weights=vec![0.;44*8];weights[0]=1.;
    v2.critic_hidden.weight=v2.critic_hidden.weight.clone().map(|_|Tensor::from_data(
        TensorData::new(weights,[44,8]),&device).require_grad());
    v2.critic.weight=v2.critic.weight.clone().map(|_|Tensor::ones([8,1],&device).require_grad());
    let mut inputs=vec![0.;3*44];inputs[0]=-1.;inputs[88]=1.;
    let values=v2.value(Tensor::from_data(TensorData::new(inputs,[3,44]),&device)).into_data().try_to_vec::<f32>().unwrap();
    assert!((values[0]+values[2]-2.*values[1]-libm::erff(1./std::f32::consts::SQRT_2)).abs()<2e-6);
    let obs=observation();
    let d=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    let p=crate::native_batch::NativeHostBatch::pack(&[&d]).upload(&device);
    let grads=model.calibrate_value_packed(&p).sum().backward();
    assert!(model.encoder.weight.grad(&grads).is_none());
    assert!(model.embedding.weight.grad(&grads).is_none());
    let v2=model.v2.as_ref().unwrap();
    assert!(v2.children.types.weight.grad(&grads).is_none());
    assert!(v2.critic_hidden.weight.grad(&grads).is_some());
    assert!(v2.critic.weight.grad(&grads).is_some());
}

#[cfg(feature="obs-v2")]
#[test]
fn published_v2_rows_use_metadata_groups_without_changing_the_legacy_mean() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();
    let model=NativePolicy::init(8,&device);
    let mut obs=observation();
    let before=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    let nodes=[
        (RowKind::CardPreview,RowHandle(7),2),
        (RowKind::EffectParameter,RowHandle(7),2),
        (RowKind::RouteSummary,RowHandle(8),3),
        (RowKind::Boss,RowHandle(1),3),
    ];
    for (kind,parent,_) in nodes {
        let mut row=Row {kind,parent,..Row::default()};
        row.numbers.set_integer(0,9).unwrap();
        obs.push_row(row).unwrap();
    }
    let preview=obs.push_row(Row {kind:RowKind::UpgradePreview,parent:RowHandle(7),
        legacy_role:LegacyRowRole::V2Only,..Row::default()}).unwrap();
    let upgraded=obs.push_row(Row {kind:RowKind::Card,parent:preview,owner:RowHandle(2),
        detail:[ContentId::ABSENT,ContentId::ABSENT,ContentId::ABSENT,
            ContentId::from_index(Namespace::PileKind,sts2_sim::combat::PileKind::None as usize).unwrap()],
        legacy_role:LegacyRowRole::V2Only,..Row::default()}).unwrap();
    obs.push_row(Row {kind:RowKind::Modifier,parent:upgraded,
        legacy_role:LegacyRowRole::V2Only,..Row::default()}).unwrap();
    obs.finish(DecisionStatus::Actionable).unwrap();
    let after=NativeDecision::from_view(obs.view().unwrap()).unwrap();
    assert_eq!(&after.groups[before.rows.len()..],&[2,2,3,3,2,2,2]);
    let p=crate::native_batch::NativeHostBatch::pack(&[&after]).upload(&device);
    let weights=p.floats.clone().slice([0..1,(p.r+p.k)*NUMERIC_LANES..(p.r+p.k)*NUMERIC_LANES+p.r])
        .into_data().try_to_vec::<f32>().unwrap();
    assert_eq!(weights.iter().filter(|&&w|w>0.).count(),before.rows.len());
    assert_eq!(after.slots,before.slots);
    let a=model.forward(&[&before],&device).logp.into_data().try_to_vec::<f32>().unwrap();
    let b=model.forward(&[&after],&device).logp.into_data().try_to_vec::<f32>().unwrap();
    assert!(a.iter().zip(b).all(|(a,b)|(a-b).abs()<2e-6));
}
