use super::*;

impl NativePolicy {
    pub fn authenticated_source_schema(manifest:&serde_json::Value)
        ->Result<serde_json::Value,Box<dyn std::error::Error>> {
        let current:serde_json::Value=serde_json::from_str(CONTENT_MANIFEST_JSON)?;
        // V1 is frozen from the compiled main observation manifest; the consumer
        // can be built against V2 without trusting a caller's column ordering.
        let v1:serde_json::Value=serde_json::from_str(include_str!("native_v1_schema.json"))?;
        let hash=manifest["schema_hash"].as_str().ok_or("missing source schema hash")?;
        for compiled in [&current,&v1] {
            if manifest["schema_version"].as_u64()==compiled["schema_version"].as_u64()
                && compiled["schema_hash"].as_str().is_some_and(|known|known.eq_ignore_ascii_case(hash)) {
                if manifest["schema"]!=compiled["schema"] {
                    return Err("source schema descriptor differs from its compiled schema".into());
                }
                return Ok(compiled["schema"].clone());
            }
        }
        Err("source schema is not a compiled V1/V2 observation schema".into())
    }
    /// Only the explicit migration path may cross manifests. Registry offsets
    /// are reconstructed from authenticated source names, never copied ordinals.
    pub fn load_legacy_schema(path:&std::path::Path,manifest:&serde_json::Value,hidden:usize,
        joint:bool,device:&Device)->Result<Self,Box<dyn std::error::Error>> {
        let schema=Self::authenticated_source_schema(manifest)?;
        if schema["numeric_lanes"].as_u64()!=Some(NUMERIC_LANES as u64) ||
            schema["id_lanes"].as_u64()!=Some(ID_LANES as u64) {
            return Err("legacy numeric/ID layout cannot be transferred".into());
        }
        let registries=manifest["registries"].as_array().ok_or("missing source registries")?;
        let flags=schema["flags"].as_array().ok_or("missing source flags")?;
        let old_width=registries.iter().filter_map(|r|r["embedding_width"].as_u64()).max()
            .ok_or("missing source embedding width")? as usize;
        let old_rows=1+registries.iter().map(|r|r["ids"].as_array().map(Vec::len)
            .ok_or("missing source content identities")).collect::<Result<Vec<_>,_>>()?.iter().sum::<usize>();
        let width=crate::native_batch::embedding_width();
        if old_width>width {return Err("source embedding width exceeds destination".into());}
        let mut old=if joint {Self::init_joint(hidden,device)}else{Self::init(hidden,device)};
        old.embedding=EmbeddingConfig::new(old_rows,old_width).init(device);
        old.encoder=LinearConfig::new(old_width+NUMERIC_LANES*4+flags.len()*2,hidden).init(device);
        let mut model=old.try_load_file(path)?;
        let source=model.embedding.weight.val().into_data().try_to_vec::<f32>()?;
        let destination=v1_content_manifest();
        let target_registries=destination["registries"].as_array().unwrap();
        let rows=manifest::v1_table_rows();
        let mut target=vec![0.;rows*width];
        target[..old_width].copy_from_slice(&source[..old_width]);
        let mut old_offset=0;
        for registry in registries {
            let ids=registry["ids"].as_array().ok_or("invalid source IDs")?;
            let mut new_offset=0;
            for new_registry in target_registries {
                if registry["name"]==new_registry["name"] {
                    for (index,id) in ids.iter().enumerate() {
                        if id["wire"].as_u64()!=Some(index as u64+1) {return Err("noncanonical source registry".into());}
                        let name=id["name"].as_str().ok_or("invalid source content name")?;
                        if let Some(new)=new_registry["ids"].as_array().unwrap().iter().position(|id|id["name"]==name) {
                            target[(new_offset+new+1)*width..][..old_width]
                                .copy_from_slice(&source[(old_offset+index+1)*old_width..][..old_width]);
                        }
                    }
                    break;
                }
                new_offset+=new_registry["count"].as_u64().unwrap() as usize;
            }
            old_offset+=ids.len();
        }
        let old_encoder=model.encoder.weight.val().into_data().try_to_vec::<f32>()?;
        let mut encoder=vec![0.;(width+NUMBERS)*hidden];
        for i in 0..old_width {
            encoder[i*hidden..][..hidden].copy_from_slice(&old_encoder[i*hidden..][..hidden]);
        }
        for i in 0..NUMERIC_LANES*4 {
            encoder[(width+i)*hidden..][..hidden].copy_from_slice(&old_encoder[(old_width+i)*hidden..][..hidden]);
        }
        let current:serde_json::Value=serde_json::from_str(CONTENT_MANIFEST_JSON)?;
        let new_flags=current["schema"]["flags"].as_array().ok_or("missing destination flags")?;
        for (old,flag) in flags.iter().enumerate() {
            if let Some(new)=new_flags.iter().position(|f|f==flag) {
                for channel in 0..2 {
                    encoder[(width+NUMERIC_LANES*4+2*new+channel)*hidden..][..hidden]
                        .copy_from_slice(&old_encoder[(old_width+NUMERIC_LANES*4+2*old+channel)*hidden..][..hidden]);
                }
            }
        }
        model.embedding.weight=model.embedding.weight.map(|_|Tensor::from_data(
            TensorData::new(target,[rows,width]),device).set_require_grad(device.is_autodiff()));
        model.encoder.weight=model.encoder.weight.map(|_|Tensor::from_data(
            TensorData::new(encoder,[width+NUMBERS,hidden]),device).set_require_grad(device.is_autodiff()));
        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_matches_namespace_content_identity_after_source_id_reordering() {
        let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
        let device=Device::flex();
        let baseline=NativePolicy::init(8,&device);
        let expected=baseline.embedding.weight.val().into_data();
        let mut source=baseline.clone();
        let mut manifest=v1_content_manifest().clone();
        let mut offset=0;
        for registry in manifest["registries"].as_array_mut().unwrap() {
            let count=registry["ids"].as_array().unwrap().len();
            if registry["name"]=="CardId" {
                let ids=registry["ids"].as_array_mut().unwrap();
                let name=ids[0]["name"].clone();ids[0]["name"]=ids[1]["name"].clone();ids[1]["name"]=name;
                break;
            }
            offset+=count;
        }
        let width=crate::native_batch::embedding_width();
        let mut reordered=source.embedding.weight.val().into_data().try_to_vec::<f32>().unwrap();
        for j in 0..width {reordered.swap((offset+1)*width+j,(offset+2)*width+j);}
        let dims=source.embedding.weight.val().dims();
        source.embedding.weight=source.embedding.weight.map(|_|Tensor::from_data(TensorData::new(reordered,dims),&device));
        let path=std::env::temp_dir().join(format!("policy-transfer-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&path).unwrap();
        source.save_file(path.join("policy.bpk")).unwrap();
        let restored=NativePolicy::load_legacy_schema(&path.join("policy.bpk"),&manifest,8,false,&device).unwrap();
        assert_eq!(restored.embedding.weight.val().into_data(),expected);
        let input=super::super::tests::public_observation(0.25);
        let p=crate::native_batch::NativeHostBatch::pack(&[&input]).upload(&device);
        assert_eq!(baseline.logits_packed(&p).0.into_data(),restored.logits_packed(&p).0.into_data());
        std::fs::remove_dir_all(path).unwrap();
    }
}
