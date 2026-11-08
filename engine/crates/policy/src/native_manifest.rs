use super::*;
use std::sync::OnceLock;

pub fn v1_content_manifest()->&'static serde_json::Value {
    static MANIFEST:OnceLock<serde_json::Value>=OnceLock::new();
    MANIFEST.get_or_init(|| {
        let frozen:serde_json::Value=serde_json::from_str(include_str!("native_v1_schema.json")).unwrap();
        let mut manifest:serde_json::Value=serde_json::from_str(CONTENT_MANIFEST_JSON).unwrap();
        let registries=manifest["registries"].as_array_mut().unwrap();
        registries.retain(|r|!matches!(r["name"].as_str(),
            Some("PublicEventTiming"|"PublicVariable"|"PublicModalKind"|"PublicModalAction")));
        for (name,key) in [("RowKind","rows"),("ActionKind","action_kinds"),("Relation","relations")] {
            let registry=registries.iter_mut().find(|r|r["name"]==name).unwrap();
            let names=frozen["schema"][key].as_array().unwrap().iter().map(|v|
                if key=="rows" {v["name"].clone()}else{v.clone()}).collect::<Vec<_>>();
            registry["count"]=names.len().into();
            registry["embedding_rows"]=(names.len()+1).into();
            registry["ids"]=names.into_iter().enumerate().map(|(index,name)|
                serde_json::json!({"index":index,"wire":index+1,"name":name})).collect();
        }
        manifest["schema"]=frozen["schema"].clone();
        manifest["schema_version"]=frozen["schema_version"].clone();
        manifest["schema_hash"]=frozen["schema_hash"].clone();
        let content=sts2_native_obs::content_hash::content_hash(&manifest);
        let hex=content.iter().map(|n|format!("{n:02x}")).collect::<String>();
        assert_eq!(hex,"1aa956aaa5d62afd852d040d0c67e2ff1eb6d1cfec6e1d736188f48e15fe0107",
            "compiled V1 vocabulary differs from main 8425a02c; explicit migration required");
        manifest["content_hash"]=hex.into();
        manifest
    })
}

pub fn observation_manifest_metadata(relational:bool,joint:bool)->serde_json::Value {
    let manifest=if relational {serde_json::from_str(CONTENT_MANIFEST_JSON).unwrap()}
        else {v1_content_manifest().clone()};
    let decode=|key:&str| {
        let text=manifest[key].as_str().unwrap();
        (0..32).map(|i|u8::from_str_radix(&text[2*i..2*i+2],16).unwrap()).collect::<Vec<_>>()
    };
    let schema=decode("schema_hash");
    let solo=serde_json::json!({"version":manifest["schema_version"],"schema":schema,"content":decode("content_hash")});
    if !joint {return solo;}
    let schema:[u8;32]=schema.try_into().unwrap();
    serde_json::json!({"solo":solo,"team_version":2,
        "team_schema":sts2_native_obs::content_hash::team_schema_hash(schema).as_slice(),
        "architecture":"actor5h_owner_pool_team_value_v1"})
}

pub fn manifest_metadata()->serde_json::Value {observation_manifest_metadata(false,false)}
pub fn joint_manifest_metadata()->serde_json::Value {observation_manifest_metadata(false,true)}
pub fn relational_manifest_metadata(joint:bool)->serde_json::Value {
    serde_json::json!({"policy_version":2,"architecture":"typed_sparse_residual_direct_seat_global_three_pools_nonlinear_value_v2",
        "action_support":POLICY_ACTION_SUPPORT,"observation":observation_manifest_metadata(true,joint)})
}
pub fn legacy_observation_mode()->String {"v1_compatibility".into()}
pub fn authenticate_observation_mode(mode:Option<&str>,relational:bool)->Result<(),String> {
    match (mode,relational) {
        (None|Some("v1_compatibility"),false)|(Some("v2"),true)=>Ok(()),
        _=>Err("observation mode does not authenticate the policy heads".into()),
    }
}
pub fn checkpoint_heads(state:&serde_json::Value)->Result<(bool,bool),String> {
    let heads=match state["schema"].as_str() {
        Some("sts2-native-policy-v3")=>(false,false),Some("sts2-native-policy-v4")=>(true,false),
        Some("sts2-native-policy-v5")=>(false,true),Some("sts2-native-policy-v6")=>(true,true),
        _=>return Err("unknown native checkpoint schema".into()),
    };
    let mode=state.get("observation_mode").map(|v|v.as_str().ok_or("invalid observation mode"))
        .transpose()?;
    authenticate_observation_mode(mode,heads.1)?;
    Ok(heads)
}
pub fn authenticate_checkpoint(state:&serde_json::Value)->Result<(bool,bool),String> {
    let (joint,relational)=checkpoint_heads(state)?;
    let expected=if relational {relational_manifest_metadata(joint)}
        else {observation_manifest_metadata(false,joint)};
    if state["manifest"]!=expected {return Err("native checkpoint manifest mismatch".into());}
    Ok((joint,relational))
}

pub(super) fn v1_table_rows()->usize {
    1+v1_content_manifest()["registries"].as_array().unwrap().iter()
        .map(|r|r["count"].as_u64().unwrap() as usize).sum::<usize>()
}
pub(super) fn v1_lookup()->&'static [i32] {
    static LOOKUP:OnceLock<Vec<i32>>=OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut lookup=vec![0;crate::native_batch::table_rows()];
        let registries=v1_content_manifest()["registries"].as_array().unwrap();
        let mut old_offset=0;
        for registry in registries {
            let mut offset=0;
            for current in REGISTRIES {
                if registry["name"]==current.name {
                    for id in registry["ids"].as_array().unwrap() {
                        let name=id["name"].as_str().unwrap();
                        let index=current.names.iter().position(|&n|n==name).unwrap();
                        lookup[offset+index+1]=(old_offset+id["wire"].as_u64().unwrap() as usize) as i32;
                    }
                    break;
                }
                offset+=current.count;
            }
            old_offset+=registry["count"].as_u64().unwrap() as usize;
        }
        lookup
    })
}

pub(super) fn v1_ids()->&'static [ContentId] {
    static IDS:OnceLock<Vec<ContentId>>=OnceLock::new();
    IDS.get_or_init(|| {
        let mut ids=vec![ContentId::ABSENT;v1_table_rows()];
        let mut current=0;
        for registry in REGISTRIES {
            for value in 1..=registry.count {
                current+=1;
                let old=v1_lookup()[current];
                if old>0 {ids[old as usize]=ContentId::from_index(registry.namespace,value-1).unwrap();}
            }
        }
        ids
    })
}
