//! Unpadded f32 inference; weights are immutable, workspaces belong to lane workers.
use super::*;
use std::{collections::HashMap,sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU64,Ordering}}};
static NEXT_POLICY:AtomicU64=AtomicU64::new(1);
#[path="native_cpu_stream.rs"]
mod stream;
static PROFILE_ENABLED:AtomicBool=AtomicBool::new(false);
static CPU_PROFILES:Mutex<Vec<Arc<CpuForwardProfile>>>=Mutex::new(Vec::new());
const CPU_COMPONENTS:[&str;6]=["state_baseline","relations","pools_critic","candidate_baseline","residual_actor","normalize"];
struct CpuForwardProfile {id:u64,relational:bool,calls:AtomicU64,nanos:AtomicU64,components:[AtomicU64;6]}
struct CpuComponentTimer {last:Option<std::time::Instant>,nanos:[u64;6]}
impl CpuComponentTimer {
    fn mark(&mut self,component:usize) {
        if let Some(last)=self.last {
            let now=std::time::Instant::now();
            self.nanos[component]+=now.duration_since(last).as_nanos() as u64;
            self.last=Some(now);
        }
    }
}
#[derive(PartialEq,Eq,Hash)]
struct FeatureKey {
    ids:[u32;ID_LANES+2], numbers:[u32;NUMERIC_LANES], ip:u16, fp:u16, flags:u64, present:u64,
}
impl From<&Features> for FeatureKey {
    fn from(f:&Features)->Self {
        Self{ids:f.ids.map(|id|((id.namespace as u32)<<16)|id.value as u32),
            numbers:f.numbers.map(f32::to_bits),ip:f.ip,fp:f.fp,flags:f.flags,present:f.present}
    }
}

struct Dense { weight:Vec<f32>, bias:Vec<f32>, input:usize, output:usize }
impl Dense {
    fn copy(layer:&Linear)->Self {
        let [input,output]=layer.weight.val().dims();
        Self{weight:layer.weight.val().into_data().try_to_vec().unwrap(),
            bias:layer.bias.as_ref().map(|x|x.val().into_data().try_to_vec().unwrap())
                .unwrap_or_else(||vec![0.;output]),input,output}
    }
    fn forward(&self,input:&[f32],output:&mut [f32]) {
        assert_eq!(input.len(),self.input);
        output.fill(0.);
        #[cfg(target_arch="x86_64")]
        if self.output>=32 && std::is_x86_feature_detected!("avx2") {
            // Feature detection guards the only unsafe entry. No FMA: keep the
            // scalar input-order multiply/add rounding exactly, in f32.
            unsafe {self.avx2(input,output);}
        }else{self.scalar(input,output);}
        #[cfg(not(target_arch="x86_64"))]
        self.scalar(input,output);
        // Burn Linear adds its bias after the matrix product. Starting the
        // reduction from bias loses it under cancellation (2^24 + 1 - 2^24).
        for (y,&bias) in output.iter_mut().zip(&self.bias) {*y+=bias;}
    }
    fn block(&self,rows:std::ops::Range<usize>,bias:bool)->Self {
        Self {weight:self.weight[rows.start*self.output..rows.end*self.output].to_vec(),
            bias:if bias {self.bias.clone()}else{vec![0.;self.output]},input:rows.len(),output:self.output}
    }
    fn scalar(&self,input:&[f32],output:&mut [f32]) {
        // Input-major Burn layout: contiguous output channels vectorize at h=128/256.
        for (&x,row) in input.iter().zip(self.weight.chunks_exact(self.output)) {
            for (y,&w) in output.iter_mut().zip(row) {*y+=x*w;}
        }
    }
    #[cfg(target_arch="x86_64")]
    #[target_feature(enable="avx2")]
    unsafe fn avx2(&self,input:&[f32],output:&mut [f32]) {
        use std::arch::x86_64::*;
        // Four output vectors stay in registers throughout the reduction.
        // Every load is in bounds: the tail uses scalar indexed channels.
        unsafe {
            let end=self.output/32*32;
            for column in (0..end).step_by(32) {
                let dst=output.as_mut_ptr().add(column);
                let (mut a,mut b,mut c,mut d)=(_mm256_loadu_ps(dst),_mm256_loadu_ps(dst.add(8)),
                    _mm256_loadu_ps(dst.add(16)),_mm256_loadu_ps(dst.add(24)));
                for (i,&x) in input.iter().enumerate() {
                    let x=_mm256_set1_ps(x);
                    let row=self.weight.as_ptr().add(i*self.output+column);
                    a=_mm256_add_ps(a,_mm256_mul_ps(x,_mm256_loadu_ps(row)));
                    b=_mm256_add_ps(b,_mm256_mul_ps(x,_mm256_loadu_ps(row.add(8))));
                    c=_mm256_add_ps(c,_mm256_mul_ps(x,_mm256_loadu_ps(row.add(16))));
                    d=_mm256_add_ps(d,_mm256_mul_ps(x,_mm256_loadu_ps(row.add(24))));
                }
                _mm256_storeu_ps(dst,a);_mm256_storeu_ps(dst.add(8),b);
                _mm256_storeu_ps(dst.add(16),c);_mm256_storeu_ps(dst.add(24),d);
            }
            for column in end..self.output {
                for (i,&x) in input.iter().enumerate() {output[column]+=x*self.weight[i*self.output+column];}
            }
        }
    }
}
pub(super) fn activate(values:&mut [f32]) {
    for x in values {
        let z=(*x/std::f32::consts::SQRT_2).abs();
        let t=1.0/(1.0+0.3275911*z);
        let p=((((1.0614054*t-1.4531521)*t+1.4214138)*t-0.28449672)*t+0.2548296)*t;
        let erf=1.0-p*(-z*z).exp();
        let erf=if *x<0. {-erf}else{erf};
        *x=0.5**x*(1.0+erf);
    }
}
pub struct CpuPolicy {
    embedding:Vec<f32>, offsets:Vec<usize>, width:usize, h:usize,
    encoder:Dense, actor_hidden:Dense, actor:Dense, critic:Dense,
    joint:bool,
    v1_lookup:Option<&'static [i32]>,
    v2:Option<CpuRelationalPolicy>,
    profile:Option<Arc<CpuForwardProfile>>,
}
struct CpuRelationalPolicy {
    children:relations::CpuChildAggregation,
    actor_blocks:[Dense;4], actor:Dense, critic_hidden:Dense, critic:Dense,
}
#[derive(Default)]
pub struct CpuWorkspace {
    encoded:HashMap<FeatureKey,usize>, encodings:Vec<f32>,
    input:Vec<f32>, state:Vec<f32>, context:Vec<f32>, candidate:Vec<f32>,
    actor_input:Vec<f32>, actor_hidden:Vec<f32>, pub logp:Vec<f32>,
    pub logits:Vec<f32>,
    owners:Vec<f32>,
    v2_input:Vec<f32>, v2_hidden:Vec<f32>,
    v2_pools:Vec<f32>,
    v2_state:Vec<f32>, v2_message:Vec<f32>, v2_degree:Vec<u64>,
    v2_projected:[Vec<bool>;3],
    v2_sources:Vec<f32>,v2_targets:Vec<f32>,v2_contexts:Vec<f32>,
    projection:Option<Box<CpuWorkspace>>,
    stream_state:Vec<f32>,stream_messages:Vec<f32>,
    stream_scratch:Vec<f32>,stream_degree:Vec<u64>,stream_row:Vec<f32>,
}
impl CpuWorkspace {
    pub fn allocation_stats(&self,hidden:usize)->serde_json::Value {
        let floats=[&self.input,&self.state,&self.context,&self.candidate,&self.actor_input,&self.encodings,
            &self.actor_hidden,&self.logp,&self.logits,&self.owners,&self.v2_input,&self.v2_hidden,
            &self.v2_pools,&self.v2_state,&self.v2_message,&self.v2_sources,&self.v2_targets,
            &self.v2_contexts,&self.stream_state,&self.stream_messages,&self.stream_scratch,&self.stream_row];
        let scratch=floats.iter().map(|v|v.capacity()*std::mem::size_of::<f32>()).sum::<usize>()
            +(self.v2_degree.capacity()+self.stream_degree.capacity())*std::mem::size_of::<u64>()
            +self.v2_projected.iter().map(|v|v.capacity()).sum::<usize>();
        serde_json::json!({"scratch_bytes":scratch,"state_rows":self.state.len()/hidden,
            "stream_state_rows":self.stream_state.len()/hidden,
            "feature_cache_entries":self.encoded.len(),"feature_cache_value_bytes":self.encodings.len()*4,
            "projection":self.projection.as_ref().map(|w|w.allocation_stats(hidden))})
    }
}
impl CpuPolicy {
    /// Opt-in timers for subsequent actor snapshots; normal inference has no clock reads.
    pub fn enable_profiling(enabled:bool) {
        PROFILE_ENABLED.store(enabled,Ordering::Relaxed);
        if !enabled {CPU_PROFILES.lock().unwrap().clear();}
    }
    pub fn take_profile()->serde_json::Value {
        let mut rows=Vec::new();
        CPU_PROFILES.lock().unwrap().retain(|profile| {
            let calls=profile.calls.swap(0,Ordering::Relaxed);
            let nanos=profile.nanos.swap(0,Ordering::Relaxed);
            let components=CPU_COMPONENTS.into_iter().zip(&profile.components).map(|(name,timer)|
                (name,timer.swap(0,Ordering::Relaxed) as f64/1e9)).collect::<std::collections::BTreeMap<_,_>>();
            if calls>0 {rows.push(serde_json::json!({"policy_id":profile.id,"relational":profile.relational,
                "calls":calls,"worker_seconds":nanos as f64/1e9,"microseconds_per_call":nanos as f64/calls as f64/1e3,
                "component_worker_s":components}));}
            Arc::strong_count(profile)>1
        });
        serde_json::Value::Array(rows)
    }
    /// The only device readbacks: once per policy version, never inside a lane.
    pub fn copy_from(model:&NativePolicy)->Self {
        let mut offset=0;
        let offsets=REGISTRIES.iter().map(|r|{let old=offset;offset+=r.count;old}).collect();
        let identity=NEXT_POLICY.fetch_add(1,Ordering::Relaxed);
        let profile=PROFILE_ENABLED.load(Ordering::Relaxed).then(|| {
            let metrics=Arc::new(CpuForwardProfile {id:identity,relational:model.is_v2(),
                calls:AtomicU64::new(0),nanos:AtomicU64::new(0),components:std::array::from_fn(|_|AtomicU64::new(0))});
            CPU_PROFILES.lock().unwrap().push(metrics.clone());metrics
        });
        Self{profile,
            embedding:model.embedding.weight.val().into_data().try_to_vec().unwrap(),
            offsets,width:crate::native_batch::embedding_width(),h:model.hidden,
            v1_lookup:(!model.is_v2()).then(manifest::v1_lookup),
            encoder:Dense::copy(&model.encoder),actor_hidden:Dense::copy(&model.actor_hidden),
            actor:Dense::copy(&model.actor),critic:Dense::copy(&model.critic),joint:model.is_joint(),
            v2:model.v2.as_ref().map(|v2| {
                let h=model.hidden;
                let actor=Dense::copy(&v2.actor_hidden);
                CpuRelationalPolicy {
                children:relations::CpuChildAggregation::copy_from(&v2.children),
                actor_blocks:[actor.block(0..h,false),actor.block(h..2*h,false),
                    actor.block(2*h..3*h,false),actor.block(3*h..8*h+4,true)],
                actor:Dense::copy(&v2.actor),
                critic_hidden:Dense::copy(&v2.critic_hidden),critic:Dense::copy(&v2.critic)}})}
    }
    fn encode(&self,f:&Features,input:&mut [f32],out:&mut [f32]) {
        input.fill(0.);
        for id in &f.ids {
            if id.value==0 {continue;}
            let row=self.offsets[id.namespace as usize-1]+id.value as usize;
            let row=self.v1_lookup.map_or(row,|lookup|lookup[row] as usize);
            if row==0 {continue;}
            for (x,&w) in input[..self.width].iter_mut().zip(&self.embedding[row*self.width..][..self.width]) {*x+=w;}
        }
        let mut column=self.width;
        for lane in 0..NUMERIC_LANES {
            let i=f.ip&(1<<lane)!=0;let p=f.fp&(1<<lane)!=0;
            input[column..column+4].copy_from_slice(&[
                if i {f.numbers[lane]}else{0.},if p {f.numbers[lane]}else{0.},
                u8::from(i) as f32,u8::from(p) as f32]);
            column+=4;
        }
        for flag in Flag::ALL {
            input[column]=u8::from(f.flags&f.present&flag.bit()!=0) as f32;
            input[column+1]=u8::from(f.present&flag.bit()!=0) as f32;
            column+=2;
        }
        self.encoder.forward(input,out);activate(out);
    }
    fn encode_cached(&self,f:&Features,input:&mut [f32],out:&mut [f32],
        cache:&mut HashMap<FeatureKey,usize>,values:&mut Vec<f32>) {
        let key=FeatureKey::from(f);
        if let Some(&offset)=cache.get(&key) {out.copy_from_slice(&values[offset..][..self.h]);return;}
        self.encode(f,input,out);
        cache.insert(key,values.len());
        values.extend_from_slice(out);
    }
    /// Returns value; legal-only log probabilities are in the reusable workspace.
    pub fn forward(&self,d:&NativeDecision,w:&mut CpuWorkspace)->f32 {
        if self.v2.is_none() {
            if let Some(old)=d.projection.as_deref() {return self.forward_mode(old,w,false,true);}
        }
        self.forward_mode(d,w,true,true)
    }
    fn forward_mode(&self,d:&NativeDecision,w:&mut CpuWorkspace,residual:bool,profile:bool)->f32 {
        let started=self.profile.as_ref().filter(|_|profile).map(|_|std::time::Instant::now());
        let mut timing=CpuComponentTimer {last:started,nanos:[0;6]};
        let v2=self.v2.as_ref().filter(|_|residual);
        let projected=if residual {d.projection.as_deref()}else{None};
        let projected_value=projected.map(|old|self.forward_mode(old,
            w.projection.get_or_insert_with(||Box::new(CpuWorkspace::default())),false,false));
        // Cache only this decision's repeated features, retaining storage rather
        // than every feature and relation message ever seen by a browser session.
        w.encoded.clear();w.encodings.clear();
        let h=self.h;
        assert!(d.actors.is_empty() || self.joint, "team observations require a joint policy checkpoint");
        w.input.resize(self.encoder.input,0.);
        w.state.resize((d.rows.len()+1)*h,0.);w.state[..h].fill(0.);
        w.context.resize(h,0.);w.context.fill(0.);
        w.owners.resize((MAX_SEATS+1)*h,0.);w.owners.fill(0.);
        let totals=d.pool_totals();
        for (i,row) in d.rows.iter().enumerate() {
            let out=&mut w.state[(i+1)*h..][..h];
            self.encode_cached(row,&mut w.input,out,&mut w.encoded,&mut w.encodings);
            stream::legacy(d,i,out,&totals,&mut w.context,&mut w.owners);
        }
        timing.mark(0);
        if let Some(v2)=v2 {
            v2.children.forward(d,&w.state,&mut w.v2_state,&mut w.v2_message,&mut w.v2_degree);
            w.v2_pools.resize(3*h,0.);w.v2_pools.fill(0.);
        }
        self.stream_forward(d,w,v2);
        timing.mark(1);
        let mut value=[projected_value.unwrap_or(0.)];
        if projected.is_none() {self.critic.forward(&w.context,&mut value);}
        if let Some(v2)=v2 {
            for (x,&y) in w.v2_state.iter_mut().zip(&w.state) {*x+=y;}
            for (row,&group) in d.groups.iter().enumerate() {
                if group==0 {continue;}
                let inv=super::weights::normalized(d.message_mass(row),totals.groups[group as usize]);
                for (x,&y) in w.v2_pools[(group as usize-1)*h..][..h].iter_mut()
                    .zip(&w.v2_state[(row+1)*h..][..h]) {*x+=y*inv;}
            }
            w.v2_input.resize(5*h+4,0.);w.v2_input.fill(0.);
            w.v2_input[..h].copy_from_slice(&w.state[d.direct.value_seat*h..][..h]);
            w.v2_input[h..2*h].copy_from_slice(&w.state[d.direct.global*h..][..h]);
            w.v2_input[2*h..5*h].copy_from_slice(&w.v2_pools);
            if let Some(ratios)=d.direct.ratios.get(d.direct.value_seat) {w.v2_input[5*h..].copy_from_slice(ratios);}
            w.v2_hidden.resize(h,0.);
            v2.critic_hidden.forward(&w.v2_input,&mut w.v2_hidden);activate(&mut w.v2_hidden);
            let mut residual=[0.];v2.critic.forward(&w.v2_hidden,&mut residual);
            value[0]+=residual[0];
            for values in [&mut w.v2_sources,&mut w.v2_targets,&mut w.v2_contexts] {values.resize(w.state.len(),0.);}
            for flags in &mut w.v2_projected {flags.resize(d.rows.len()+1,false);flags.fill(false);}
            timing.mark(2);
        }
        w.candidate.resize(h,0.);w.actor_input.resize(if self.joint {5*h}else{4*h},0.);
        w.actor_hidden.resize(h,0.);w.logp.resize(d.slots.len(),0.);
        timing.mark(0);
        for (i,candidate) in d.candidates.iter().enumerate() {
            self.encode_cached(candidate,&mut w.input,&mut w.candidate,&mut w.encoded,&mut w.encodings);
            w.actor_input[..h].copy_from_slice(&w.candidate);
            w.actor_input[h..2*h].copy_from_slice(&w.context);
            w.actor_input[2*h..3*h].copy_from_slice(&w.state[d.sources[i]*h..][..h]);
            w.actor_input[3*h..4*h].copy_from_slice(&w.state[d.targets[i]*h..][..h]);
            if self.joint {
                let owner=d.actors.get(i).copied().unwrap_or(0) as usize;
                w.actor_input[4*h..5*h].copy_from_slice(&w.owners[owner*h..][..h]);
            }
            if projected.is_some() {w.logp[i]=w.projection.as_ref().unwrap().logits[i];}
            else {
                self.actor_hidden.forward(&w.actor_input,&mut w.actor_hidden);activate(&mut w.actor_hidden);
                self.actor.forward(&w.actor_hidden,&mut w.logp[i..i+1]);
            }
            timing.mark(3);
            if let Some(v2)=v2 {
                let seat=d.direct.seats.get(i).copied().unwrap_or(0);
                w.v2_hidden.resize(h,0.);
                v2.actor_blocks[0].forward(&w.candidate,&mut w.v2_hidden);
                // Factor the same linear map; shared context and repeated
                // endpoints need one product, not one per legal candidate.
                for (part,handle) in [d.sources[i],d.targets[i],seat].into_iter().enumerate() {
                    let values=match part {0=>&mut w.v2_sources,1=>&mut w.v2_targets,_=>&mut w.v2_contexts};
                    let projected=&mut values[handle*h..][..h];
                    if !w.v2_projected[part][handle] {
                        if part==2 {
                            w.v2_input.resize(5*h+4,0.);w.v2_input.fill(0.);
                            w.v2_input[..h].copy_from_slice(&w.state[seat*h..][..h]);
                            w.v2_input[h..2*h].copy_from_slice(&w.state[d.direct.global*h..][..h]);
                            w.v2_input[2*h..5*h].copy_from_slice(&w.v2_pools);
                            if let Some(ratios)=d.direct.ratios.get(seat) {w.v2_input[5*h..].copy_from_slice(ratios);}
                            v2.actor_blocks[3].forward(&w.v2_input,projected);
                        }else{
                            v2.actor_blocks[part+1].forward(&w.v2_state[handle*h..][..h],projected);
                        }
                        w.v2_projected[part][handle]=true;
                    }
                    for (x,&y) in w.v2_hidden.iter_mut().zip(projected.iter()) {*x+=y;}
                }
                activate(&mut w.v2_hidden);
                let mut residual=[0.];v2.actor.forward(&w.v2_hidden,&mut residual);
                w.logp[i]+=residual[0];
            }
            timing.mark(4);
        }
        w.logits.clone_from(&w.logp);
        let max=w.logp.iter().copied().fold(f32::NEG_INFINITY,f32::max);
        let denominator=w.logp.iter().map(|x|(*x-max).exp()).sum::<f32>().ln();
        for x in &mut w.logp {*x=(*x-max)-denominator;}
        timing.mark(5);
        if let Some(started)=started {
            let profile=self.profile.as_ref().unwrap();
            profile.nanos.fetch_add(started.elapsed().as_nanos() as u64,Ordering::Relaxed);
            for (counter,nanos) in profile.components.iter().zip(timing.nanos) {counter.fetch_add(nanos,Ordering::Relaxed);}
            profile.calls.fetch_add(1,Ordering::Relaxed);
        }
        value[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiling_counts_calls_without_changing_predictions() {
        let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
        let model=NativePolicy::init_v2(8,&Device::flex());
        let mut cpu=CpuPolicy::copy_from(&model);
        let input=super::super::tests::public_observation(0.25);
        let mut workspace=CpuWorkspace::default();
        let expected=cpu.forward(&input,&mut workspace);
        let expected_logp=workspace.logp.clone();
        let metrics=Arc::new(CpuForwardProfile {id:NEXT_POLICY.fetch_add(1,Ordering::Relaxed),relational:true,
            calls:AtomicU64::new(0),nanos:AtomicU64::new(0),components:std::array::from_fn(|_|AtomicU64::new(0))});
        cpu.profile=Some(metrics.clone());
        for _ in 0..2 {
            assert_eq!(cpu.forward(&input,&mut workspace).to_bits(),expected.to_bits());
            assert_eq!(workspace.logp,expected_logp);
        }
        assert_eq!(metrics.calls.load(Ordering::Relaxed),2);
    }
    #[test]
    fn blocked_dense_preserves_scalar_f32_bits_including_tails() {
        for output in [1,8,33,128,256] {
            let input=107;
            let dense=Dense{weight:(0..input*output).map(|i|((i*37%97) as f32-48.)/97.).collect(),
                bias:vec![0.17;output],input,output};
            let x:Vec<_>=(0..input).map(|i|(i as f32-53.)/29.).collect();
            let mut expected=vec![0.;output];dense.scalar(&x,&mut expected);
            for (y,&bias) in expected.iter_mut().zip(&dense.bias) {*y+=bias;}
            let mut actual=vec![0.;output];dense.forward(&x,&mut actual);
            assert_eq!(actual.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),
                expected.iter().map(|v|v.to_bits()).collect::<Vec<_>>());
        }
    }
    #[test]
    fn bias_is_added_after_cancelling_products_for_scalar_and_simd_widths() {
        for output in [1,8,33,128,256] {
            let dense=Dense{weight:[vec![16777216.;output],vec![-16777216.;output]].concat(),
                bias:vec![1.;output],input:2,output};
            let mut actual=vec![0.;output];dense.forward(&[1.,1.],&mut actual);
            assert!(actual.iter().all(|v|v.to_bits()==1f32.to_bits()));
        }
    }
    #[test]
    fn plain_forward_matches_burn_ragged_realistic_widths_and_handles() {
        compare_parity(Device::flex(),"CPU",2e-5);
    }
    fn compare_parity(device:Device,backend:&str,tolerance:f32) {
        let _guard=crate::RNG_TEST_LOCK.lock().unwrap();
        for h in [8,128,256] {
          for relational in [false,true] {
            device.seed(20261030);
            let model=if relational {NativePolicy::init_v2(h,&device)}else{NativePolicy::init(h,&device)};
            let cpu=CpuPolicy::copy_from(&model);
            let input=||NativeDecision::from_view(super::super::v2_tests::observation().view().unwrap()).unwrap();
            let mut a=if relational {input()}else{super::super::tests::public_observation(-0.375)};
            a.sources[0]=1;a.targets[1]=2;
            a.rows[0].ids[2]=a.candidates[0].ids[1];
            a.rows[0].ids[3]=a.rows[0].ids[2];
            a.rows[0].ip=3;a.rows[0].fp=2;
            a.rows[0].numbers[0]=-3.;a.rows[0].numbers[1]=7.;
            a.rows[0].present=u64::MAX;a.rows[0].flags=0xa5;
            let mut b=if relational {input()}else{super::super::tests::public_observation(100.)};
            b.rows.extend(a.rows.clone());
            let packed=crate::native_batch::NativeHostBatch::pack(&[&a,&b]).upload(&device);
            let result=model.forward_packed(&packed);
            let k=result.logp.dims()[1];
            let lp=result.logp.into_data().try_to_vec::<f32>().unwrap();
            let values=result.value.into_data().try_to_vec::<f32>().unwrap();
            let mut workspace=CpuWorkspace::default();
            let (mut max_value,mut max_logp)=(0f32,0f32);
            for (i,d) in [&a,&b].into_iter().enumerate() {
                let value=cpu.forward(d,&mut workspace);
                max_value=max_value.max((value-values[i]).abs());
                assert!((value-values[i]).abs()<tolerance,"h={h}: value {value} vs {}",values[i]);
                for (j,&p) in workspace.logp.iter().enumerate() {
                    max_logp=max_logp.max((p-lp[i*k+j]).abs());
                    assert!((p-lp[i*k+j]).abs()<tolerance,"h={h}: logp {p} vs {}",lp[i*k+j]);
                }
                assert!((workspace.logp.iter().map(|p|p.exp()).sum::<f32>()-1.).abs()<1e-5);
            }
            eprintln!("actor_parity backend={backend} relational={relational} hidden={h} max_abs_value={max_value:e} max_abs_logp={max_logp:e} tolerance={tolerance:e}");
            // Reusing the workspace with different weights cannot reuse cached encodings.
            device.seed(99);
            let other=if relational {NativePolicy::init_v2(h,&device)}else{NativePolicy::init(h,&device)};
            let other_cpu=CpuPolicy::copy_from(&other);
            let expected=other.forward(&[&a],&device).value.into_data().try_to_vec::<f32>().unwrap()[0];
            let actual=other_cpu.forward(&a,&mut workspace);
            assert!((actual-expected).abs()<tolerance);
          }
        }
    }
}
