use super::*;
use sts2_sim::{combat::Combat,rng::Rng,run::{Coord,RunManager,RunOptions,
    mp::{Character,MpPhase,MpRun,PlayerSpec}},tables::*};

fn capture(obs:&NativeObsV1)->(NativeDecision,NativeDecision) {
    let view=obs.view().unwrap();
    let old=view.legacy.expect("real-state exact V1 sidecar");
    let original=NativeDecision::from_view(NativeDecision::projection_view(old,view.identity)).unwrap();
    let current=NativeDecision::from_view(view).unwrap();
    assert_eq!(current.slots,original.slots);
    assert!(current.rows.len()!=original.rows.len() || current.targets!=original.targets);
    (current,original)
}
fn real_cases()->Vec<(NativeDecision,NativeDecision)> {
    let mut cases=Vec::new();
    for kind in [MapPointType::Shop,MapPointType::RestSite] {
        let specs=[PlayerSpec {net_id:10,character:Character::Ironclad},
            PlayerSpec {net_id:11,character:Character::Silent}];
        let mut run=MpRun::new("POLICY-LEGACY-PROJECTION",&specs,10).unwrap();
        for seat in 0..2 {
            let player=run.roster.player_mut(seat).unwrap();
            player.gold=500;player.hp=77;player.max_hp=80;
            let mut relic=sts2_sim::relic::Relic::new(RelicId::RegalPillow);
            relic.owner=seat as u8;player.relics.push(relic);
        }
        run.phase=MpPhase::Map;
        let (x,y)=run.world.run.coord.unwrap();
        let child=run.world.map.point(Coord::new(x,y)).unwrap().children[0] as usize;
        run.world.map.points[child].point_type=kind;
        let coord=run.world.map.points[child].coord;
        for seat in 0..2 {run.vote(10+seat as u64,seat,coord).unwrap();}
        let mut encoder=NativeEncoder::new().with_legacy_projection();
        let mut obs=NativeObsV1::new();
        for seat in 0..2 {
            encoder.encode_multiplayer(&run,DecisionId {episode:7,generation:0,actor:seat as u8},
                10+seat as u64,&mut obs).unwrap();
            cases.push(capture(&obs));
        }
    }
    let mut ctx=RunManager::new(7,RunOptions::default()).run_ctx().clone();
    ctx.deck.clear();ctx.relics.clear();ctx.potions.clear();ctx.max_potion_slots=0;
    let mut combat=Combat::new(ctx,7);
    combat.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    combat.players[0].hand.clear();combat.players[0].draw.clear();combat.players[0].discard.clear();
    let card=combat.new_card(0,CardId::BodySlam,false).unwrap();
    combat.players[0].hand.push(card);combat.cr_mut(0).block=17;
    let mut obs=NativeObsV1::new();
    NativeEncoder::new().with_legacy_projection().encode_combat_snapshot(&combat,&[Character::Ironclad],
        DecisionId {episode:7,generation:0,actor:0},1,&mut obs).unwrap();
    cases.push(capture(&obs));
    cases
}

#[test]
fn exact_projection_preserves_real_state_baseline_through_serialization_and_batch_transport() {
    let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
    let device=Device::flex();
    let old=NativePolicy::init(8,&device);
    let new=old.clone().into_relational_warm_start(&device).unwrap();
    let cases=real_cases();
    let mut current=cases.iter().map(|(input,_)|
        serde_json::from_slice::<NativeDecision>(&serde_json::to_vec(input).unwrap()).unwrap()).collect::<Vec<_>>();
    let mut originals=cases.iter().map(|(_,original)|original.clone()).collect::<Vec<_>>();
    current.push(originals[0].clone());originals.push(originals[0].clone());
    let refs=current.iter().collect::<Vec<_>>();
    let packed=crate::native_batch::NativeHostBatch::pack(&refs).upload(&device);
    let permutation=(0..current.len()).rev().map(|i|i as i32).collect::<Vec<_>>();
    let packed=packed.select(Tensor::<1,Int>::from_data(TensorData::new(permutation,[current.len()]),&device));
    let cpu=CpuPolicy::copy_from(&new);
    let mut workspace=CpuWorkspace::default();
    for (lane,index) in (0..current.len()).rev().enumerate() {
        let original=crate::native_batch::NativeHostBatch::pack(&[&originals[index]]).upload(&device);
        let (expected,value)=old.logits_packed(&original);
        let expected=expected.into_data().try_to_vec::<f32>().unwrap();
        let value=value.into_data().try_to_vec::<f32>().unwrap()[0];
        let (actual,actual_value)=new.logits_packed(&packed.slice(lane,lane+1));
        let actual=actual.into_data().try_to_vec::<f32>().unwrap();
        for i in 0..current[index].slots.len() {assert!((actual[i]-expected[i]).abs()<2e-5);}
        assert!((actual_value.into_data().try_to_vec::<f32>().unwrap()[0]-value).abs()<2e-5);
        assert!((cpu.forward(&current[index],&mut workspace)-value).abs()<2e-5);
        for i in 0..current[index].slots.len() {assert!((workspace.logits[i]-expected[i]).abs()<2e-5);}
    }
}
