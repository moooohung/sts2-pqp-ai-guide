use super::*;

#[test]
fn joint_web_weights_and_action_identities_round_trip_with_ordered_queue_frames() {
    let model=NativePolicy::init_joint(8,&Device::flex());
    let meta=WebPolicyMeta {lineage:"joint-test".into(),update:0,step:0,checkpoint_id:"joint-test.0".into(),
        checkpoint_sha256:sha256(b"joint"),hidden:8,reward:Reward::default(),game_version:"test".into()};
    let bytes=export(&model,&meta).unwrap();
    let mut ai=NativeAiPlay::new(bytes).unwrap();
    let header:Value=serde_json::from_str(&ai.start(202610290000,
        r#"{"party":["IRONCLAD","NECROBINDER"],"ascension":10,"frames":true}"#).unwrap()).unwrap();
    assert_eq!(header["v"],3);
    let mut combat=false;
    let mut sequence=None;
    for _ in 0..64 {
        let chunk:Value=serde_json::from_str(&ai.advance(1).unwrap()).unwrap();
        assert_eq!(chunk["fault"],"");
        for frame in chunk["frames"].as_array().unwrap() {
            assert_eq!(frame["actor"],frame["a"]["actor"]);
            assert_eq!(frame["after"]["players"].as_array().unwrap().len(),2);
            for event in frame["queue_events"].as_array().unwrap() {
                let next=event["seq"].as_u64().unwrap();
                if let Some(previous)=sequence {assert_eq!(next,previous+1);}
                sequence=Some(next);
                if event["action_id"].is_null() {assert!(event["actor"].is_null());}
            }
            if frame["ph"]=="COMBAT" {
                let actors:std::collections::BTreeSet<_>=frame["opts"].as_array().unwrap().iter()
                    .map(|option|option["actor"].as_u64().unwrap()).collect();
                assert_eq!(actors.len(),2);
                combat=true;
            }
        }
        if combat {break;}
    }
    assert!(combat);
    assert!(sequence.is_some());
    let episode=ai.episode.as_ref().unwrap();
    for line in episode.rows.lines() {
        let row:crate::native_verify::Row=serde_json::from_str(line).unwrap();
        let identity=row.action.identity.unwrap();
        assert_eq!(row.actor,identity.actor);
        assert_eq!(row.action.candidate_slot as i32,identity.slot);
        assert_eq!(identity.contract,ACTION_CONTRACT);
    }
}
