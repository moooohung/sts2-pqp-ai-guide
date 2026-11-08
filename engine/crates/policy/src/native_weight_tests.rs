use super::*;

#[derive(Clone,Copy)]
enum Form {Expanded,Grouped,Streamed}
fn decision(groups:usize,multiplicity:u32,form:Form,team:bool,reference:Option<bool>)->NativeDecision {
    let mut obs=super::v2_tests::observation();
    let pile_kind=ContentId::from_index(Namespace::PileKind,sts2_sim::combat::PileKind::Draw as usize).unwrap();
    let mut summary=Row {kind:RowKind::PileSummary,parent:RowHandle(2),owner:RowHandle(2),
        legacy_role:LegacyRowRole::V2Only,..Row::default()};
    summary.detail[0]=pile_kind;
    summary.numbers.set_integer(0,(groups as u32*multiplicity) as i32).unwrap();
    let pile=obs.push_row(summary).unwrap();
    let mut rows=Vec::new();let mut edges=Vec::new();let mut spans=Vec::new();
    for group in 0..groups {
        let mut card=Row {kind:RowKind::Card,parent:RowHandle(2),owner:RowHandle(2),
            model:ContentId::from_index(Namespace::CardId,0).unwrap(),..Row::default()};
        card.detail[3]=pile_kind;card.numbers.set_integer(0,1).unwrap();
        let mut child=Row {kind:RowKind::Modifier,owner:RowHandle(2),..Row::default()};
        child.numbers.set_integer(0,group as i32+1).unwrap();
        match form {
            Form::Expanded|Form::Grouped=>{
                let copies=if matches!(form,Form::Expanded) {multiplicity}else{1};
                for _ in 0..copies {
                    card.multiplicity=if matches!(form,Form::Grouped) {multiplicity}else{1};
                    child.multiplicity=card.multiplicity;
                    let root=obs.push_row(card).unwrap();child.parent=root;obs.push_row(child).unwrap();
                    obs.push_edge(Edge {from:pile,to:root,relation:Relation::Member}).unwrap();
                }
            }
            Form::Streamed=>{
                card.multiplicity=multiplicity;child.multiplicity=multiplicity;
                child.parent=RowHandle(PILE_LOCAL_BASE+1);
                let row_start=rows.len();let edge_start=edges.len();
                rows.extend([card,child]);
                edges.push(Edge {from:pile,to:RowHandle(PILE_LOCAL_BASE+1),relation:Relation::Member});
                spans.push(PileGroupSpan {pile,row_start,row_len:2,edge_start,edge_len:1,multiplicity});
            }
        }
    }
    if let Some(reference)=reference {
        let choice=obs.push_row(Row {kind:RowKind::Choice,owner:RowHandle(2),..Row::default()}).unwrap();
        obs.push_row(Row {kind:RowKind::ChoiceOption,parent:choice,owner:RowHandle(2),..Row::default()}).unwrap();
        if reference {
            let mut card=Row {kind:RowKind::Card,parent:choice,owner:RowHandle(2),
                model:ContentId::from_index(Namespace::CardId,0).unwrap(),..Row::default()};
            card.detail[3]=pile_kind;card.numbers.set_integer(0,1).unwrap();
            let card=obs.push_row(card).unwrap();
            let mut child=Row {kind:RowKind::Modifier,parent:card,owner:RowHandle(2),..Row::default()};
            child.numbers.set_integer(0,1).unwrap();obs.push_row(child).unwrap();
        }
    }
    obs.finish(DecisionStatus::Actionable).unwrap();
    let mut view=obs.view().unwrap();
    if team {view.scope=DecisionScope::TeamCombat;}
    if matches!(form,Form::Streamed) {
        view.streamed_piles=StreamedPilesView {groups:&spans,rows:&rows,edges:&edges};
    }
    NativeDecision::from_view(view).unwrap()
}

fn check(device:Device) {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    for (groups,team) in [(1,false),(600,false),(2,true)] {
        let model=if team {NativePolicy::init_joint_v2(8,&device)}else{NativePolicy::init_v2(8,&device)};
        let cpu=CpuPolicy::copy_from(&model);
        let inputs=[decision(groups,3,Form::Expanded,team,None),decision(groups,3,Form::Grouped,team,None),
            decision(groups,3,Form::Streamed,team,None)];
        let encoded=inputs.iter().map(|input|
            serde_json::from_slice::<NativeDecision>(&serde_json::to_vec(input).unwrap()).unwrap()).collect::<Vec<_>>();
        let packed=crate::native_batch::NativeHostBatch::pack(&encoded.iter().collect::<Vec<_>>()).upload(&device);
        let permutation=Tensor::<1,Int>::from_data(TensorData::new(vec![2,0,1],[3]),&device);
        let packed=packed.select(permutation);
        let reference=crate::native_batch::NativeHostBatch::pack(&[&inputs[0]]).upload(&device);
        let (expected,value)=model.logits_packed(&reference);
        let expected=expected.into_data().try_to_vec::<f32>().unwrap();
        let value=value.into_data().try_to_vec::<f32>().unwrap()[0];
        let mut workspace=CpuWorkspace::default();
        for (lane,index) in [2,0,1].into_iter().enumerate() {
            let (actual,actual_value)=model.logits_packed(&packed.slice(lane,lane+1));
            let actual=actual.into_data().try_to_vec::<f32>().unwrap();
            let error=actual.iter().take(inputs[index].slots.len()).zip(&expected)
                .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error<2e-4,"groups={groups},form={index},raw error={error}");
            assert!((actual_value.into_data().try_to_vec::<f32>().unwrap()[0]-value).abs()<2e-4);
            assert!((cpu.forward(&inputs[index],&mut workspace)-value).abs()<2e-4);
            for (a,b) in workspace.logits.iter().zip(&expected) {assert!((a-b).abs()<2e-4);}
        }
        let actual=model.forward(&[&inputs[2]],&device).value.sum().backward();
        let expected=model.forward(&[&inputs[0]],&device).value.sum().backward();
        let a=model.v2.as_ref().unwrap().children.types.weight.grad(&actual).unwrap().into_data().try_to_vec::<f32>().unwrap();
        let b=model.v2.as_ref().unwrap().children.types.weight.grad(&expected).unwrap().into_data().try_to_vec::<f32>().unwrap();
        let error=a.iter().zip(&b).map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
        assert!(error<2e-4,"groups={groups},type gradient error={error}");
        eprintln!("weighted_expansion groups={groups} team={team} backend={device:?} max_abs_type_grad={error:e}");
    }
}
#[test]
fn grouped_and_streamed_facts_match_expanded_policy_values_and_gradients() {check(Device::flex().autodiff());}
#[test]
fn choice_details_do_not_duplicate_pile_weight_in_actor_or_critic() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();
    let model=NativePolicy::init(8,&device);
    let cpu=CpuPolicy::copy_from(&model);
    for form in [Form::Grouped,Form::Streamed] {
        let before=decision(1,3,form,false,Some(false));
        let after=decision(1,3,form,false,Some(true));
        let mut a=CpuWorkspace::default();let mut b=CpuWorkspace::default();
        let value_a=cpu.forward(&before,&mut a);let value_b=cpu.forward(&after,&mut b);
        assert!((value_a-value_b).abs()<2e-6,"choice reference changed weighted baseline value");
        for (a,b) in a.logits.iter().zip(&b.logits) {
            assert!((a-b).abs()<2e-6,"choice reference double-counted the pile in actor context");
        }
        let a=model.forward(&[&before],&device);let b=model.forward(&[&after],&device);
        let value_a=a.value.into_data().try_to_vec::<f32>().unwrap()[0];
        let value_b=b.value.into_data().try_to_vec::<f32>().unwrap()[0];
        assert!((value_a-value_b).abs()<2e-6);
        for (a,b) in a.logp.into_data().try_to_vec::<f32>().unwrap().iter()
            .zip(b.logp.into_data().try_to_vec::<f32>().unwrap()) {
            assert!((a-b).abs()<2e-6);
        }
    }
}
#[test]
fn zero_materialized_multiplicity_is_rejected() {
    let obs=super::v2_tests::observation();
    let mut view=obs.view().unwrap();
    let mut multiplicity=view.rows.multiplicity.to_vec();multiplicity[0]=0;
    view.rows.multiplicity=&multiplicity;
    assert!(NativeDecision::from_view(view).unwrap_err().contains("multiplicity"));
}
