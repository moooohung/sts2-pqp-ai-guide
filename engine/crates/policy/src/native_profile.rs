use super::*;
use crate::native_batch::NativeBatch;
use std::time::Instant;

fn measure(device:&Device,repeats:usize,mut forward:impl FnMut()->Tensor<1>)->serde_json::Value {
    let (mut fwd,mut bwd)=(Vec::new(),Vec::new());
    for iteration in 0..repeats+2 {
        device.sync().unwrap();
        let start=Instant::now();
        let loss=forward();
        device.sync().unwrap();
        let forward_ms=start.elapsed().as_secs_f64()*1000.;
        let start=Instant::now();
        let gradients=loss.backward();
        device.sync().unwrap();
        let backward_ms=start.elapsed().as_secs_f64()*1000.;
        drop(gradients);
        if iteration>=2 {fwd.push(forward_ms);bwd.push(backward_ms);}
    }
    let mean=|xs:&[f64]|xs.iter().sum::<f64>()/xs.len() as f64;
    serde_json::json!({"forward_ms":mean(&fwd),"backward_ms":mean(&bwd),
        "total_ms":mean(&fwd)+mean(&bwd),"forward_samples_ms":fwd,"backward_samples_ms":bwd})
}

impl NativePolicy {
    /// Intrusive component timings, including input gradients but excluding
    /// upstream graphs and Adam. No profiling hook runs in normal inference.
    pub fn profile_components(&self,p:&NativeBatch,repeats:usize)->serde_json::Value {
        assert!(repeats>0 && repeats<=16);
        let v2=self.v2.as_ref().expect("component profiling requires a relational policy");
        let device=self.embedding.weight.val().device().autodiff();
        let h=self.hidden;
        let frozen=self.valid();
        let all=frozen.encode_packed(p).reshape([p.b,p.r+p.k,h]);
        let state=Tensor::cat(vec![Tensor::<3>::zeros([p.b,1,h],&all.device()),
            all.clone().slice([0..p.b,0..p.r,0..h])],1);
        let candidates=all.clone().slice([0..p.b,p.r..p.r+p.k,0..h]);
        let entities=frozen.v2.as_ref().unwrap().children.forward(p,state.clone())+state.clone();
        let pools=v2.pools(p,entities.clone());
        let leaf=|x:Tensor<3>|x.autodiff().detach().require_grad();
        let mut result=serde_json::Map::new();
        result.insert("encoder".into(),measure(&device,repeats,||self.encode_packed(p).sum()));
        result.insert("legacy_actor_critic".into(),measure(&device,repeats,|| {
            let (_,_,scores,value)=self.baseline_packed(p,leaf(all.clone()),None);
            scores.sum()+value.sum()
        }));
        result.insert("relations".into(),measure(&device,repeats,||
            v2.children.forward(p,leaf(state.clone())).sum()));
        result.insert("pools".into(),measure(&device,repeats,||v2.pools(p,leaf(entities.clone())).sum()));
        result.insert("residual_actor".into(),measure(&device,repeats,|| {
            let features=v2.actor_features(p,leaf(candidates.clone()),leaf(state.clone()),
                leaf(entities.clone()),leaf(pools.clone()));
            v2.actor.forward(gelu(v2.actor_hidden.forward(features))).sum()
        }));
        result.insert("relational_critic".into(),measure(&device,repeats,||
            v2.value(v2.value_features(p,leaf(state.clone()),leaf(pools.clone()))).sum()));
        serde_json::Value::Object(result)
    }
}
