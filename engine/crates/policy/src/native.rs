//! Keep the transferred branch on original public inputs when V2 corrects their values.
use burn::{module::Module, nn::{Embedding, EmbeddingConfig, Linear, LinearConfig},
    prelude::*, tensor::{TensorData, activation::gelu}};
use sts2_native_obs::*;
use crate::packed::PackedOut;

const NUMBERS: usize = NUMERIC_LANES * 4 + Flag::COUNT * 2;
pub const POLICY_ACTION_SUPPORT:&str="monotone_multiselect_original_faults_v2";
/// Plain heads consume a single V1 pass; relational heads consume normal V2.
/// Neither production path requests the transfer-only projection sidecar.
pub fn encoder_for_policy(relational:bool)->NativeEncoder {
    #[cfg(feature="obs-v2")]
    if !relational {return NativeEncoder::v1_compatibility();}
    #[cfg(not(feature="obs-v2"))]
    let _=relational;
    NativeEncoder::new()
}
/// Transfer validation alone needs the original-layout encoder pass.
pub fn encoder_for_transfer()->NativeEncoder {
    #[cfg(feature="obs-v2")]
    {NativeEncoder::new().with_legacy_projection()}
    #[cfg(not(feature="obs-v2"))]
    {NativeEncoder::new()}
}
#[path = "native_cpu.rs"]
mod cpu;
pub use cpu::{CpuPolicy, CpuWorkspace};
#[path="native_relations.rs"]
mod relations;
#[path="native_v2.rs"]
mod v2;
#[path="native_transfer.rs"]
mod transfer;
#[path="native_manifest.rs"]
mod manifest;
pub use manifest::{v1_content_manifest,observation_manifest_metadata,manifest_metadata,joint_manifest_metadata,
    relational_manifest_metadata,legacy_observation_mode,authenticate_observation_mode,checkpoint_heads,authenticate_checkpoint};
pub use checkpoint_ids::with_checkpoint_observation_mode;
#[path="native_profile.rs"]
mod profile;
#[path="native_weight.rs"]
pub(crate) mod weights;
#[path="native_stream.rs"]
mod streams;
#[path="native_memory.rs"]
mod memory;

/// Finite native f32/i32 inputs map to [-ln(1+MAX), ln(1+MAX)].
/// Presence channels remain independent, including known zero.
fn scaled_number(value: f32) -> f32 {
    // Observation hashes require identical bits, not platform libm's f32 rounding.
    let magnitude=libm::log1p(f64::from(value.abs())) as f32;
    value.signum()*magnitude
}

/// Exactly zero probability and score gradient outside the legal support.
/// At least one legal candidate is guaranteed by actionable NativeObsV1.
fn masked_log_probs(scores: Tensor<2>, invalid: Tensor<2, Bool>) -> Tensor<2> {
    let legal = scores.mask_fill(invalid.clone(), f32::NEG_INFINITY);
    let max = legal.clone().max_dim(1).detach();
    let shifted = (legal - max).mask_fill(invalid.clone(), 0.);
    let denominator = shifted.clone().exp().mask_fill(invalid.clone(), 0.).sum_dim(1).log();
    (shifted - denominator).mask_fill(invalid, f32::NEG_INFINITY)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Features {
    #[serde(with="checkpoint_ids")]
    pub(crate) ids: [ContentId; ID_LANES + 2],
    pub(crate) numbers: [f32; NUMERIC_LANES],
    pub(crate) ip: u16,
    pub(crate) fp: u16,
    pub(crate) flags: u64,
    pub(crate) present: u64,
}

impl Features {
    fn new(ids: impl AsRef<[ContentId]>, integers: &[i32; NUMERIC_LANES],
        floats: &[f32; NUMERIC_LANES], ip: u16, fp: u16, flags: u64, present: u64) -> Self {
        let mut numbers = [0.; NUMERIC_LANES];
        for lane in 0..NUMERIC_LANES {
            let i = ip & (1 << lane) != 0;
            let f = fp & (1 << lane) != 0;
            numbers[lane] = if i { scaled_number(integers[lane] as f32) } else { 0. }
                + if f { scaled_number(floats[lane]) } else { 0. };
        }
        let mut fixed_ids = [ContentId::ABSENT; ID_LANES + 2];
        fixed_ids[..ids.as_ref().len()].copy_from_slice(ids.as_ref());
        Self { ids:fixed_ids, numbers, ip, fp, flags, present }
    }
    #[cfg(test)]
    fn numeric_channels(&self) -> Vec<f32> {
        let mut numeric = Vec::with_capacity(NUMBERS);
        for lane in 0..NUMERIC_LANES {
            let i = self.ip & (1 << lane) != 0;
            let f = self.fp & (1 << lane) != 0;
            numeric.extend([if i {self.numbers[lane]} else {0.},
                if f {self.numbers[lane]} else {0.},u8::from(i) as f32,u8::from(f) as f32]);
        }
        for flag in Flag::ALL {
            numeric.extend([u8::from(self.flags & self.present & flag.bit() != 0) as f32,
                u8::from(self.present & flag.bit() != 0) as f32]);
        }
        numeric
    }
}

/// Owned learned inputs only; neither absolute identities nor bindings enter tensors.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NativeDecision {
    pub(crate) rows: Vec<Features>,
    pub(crate) candidates: Vec<Features>,
    pub(crate) sources: Vec<usize>,
    pub(crate) targets: Vec<usize>,
    pub slots: Vec<i32>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) actors: Vec<u8>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) owners: Vec<u8>,
    /// Observation-local endpoints and the schema's relation wire ID, never content identities.
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) relations: Vec<[usize; 3]>,
    #[serde(default)]
    pub(crate) direct: DirectContext,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) groups: Vec<u8>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) legacy: Vec<bool>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub(crate) projection: Option<Box<NativeDecision>>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) multiplicity:Vec<u32>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) pool_multiplicity:Vec<u32>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) message_multiplicity:Vec<u32>,
    #[serde(default, skip_serializing_if="Vec::is_empty")]
    pub(crate) degree:Vec<u64>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub(crate) totals:Option<weights::PoolTotals>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub(crate) streamed:Option<Box<streams::StreamedFacts>>,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct DirectContext {
    pub(crate) global: usize,
    pub(crate) value_seat: usize,
    pub(crate) seats: Vec<usize>,
    pub(crate) ratios: Vec<[f32; 4]>,
}
mod checkpoint_ids {
    use super::*;
    use serde::{Deserialize,Serialize};
    thread_local! {static RELATIONAL:std::cell::Cell<bool>=const {std::cell::Cell::new(true)};}
    pub fn with_checkpoint_observation_mode<R>(relational:bool,read:impl FnOnce()->R)->R {
        struct Restore(bool);
        impl Drop for Restore {fn drop(&mut self){RELATIONAL.set(self.0);}}
        let _restore=Restore(RELATIONAL.replace(relational));
        read()
    }
    pub fn serialize<S:serde::Serializer>(ids:&[ContentId;ID_LANES+2],s:S)->Result<S::Ok,S::Error> {
        if RELATIONAL.get() {
            return ids.map(|id|(id.namespace as usize,id.value)).serialize(s);
        }
        let registries=v1_content_manifest()["registries"].as_array().unwrap();
        let mut wire=Vec::with_capacity(ids.len());
        for id in ids {
            if *id==ContentId::ABSENT {wire.push((0usize,0u16));continue;}
            let mut offset=0;
            let mut old=None;
            for registry in REGISTRIES {
                if registry.namespace==id.namespace {
                    if id.value>0 && id.value as usize<=registry.count {
                        old=manifest::v1_lookup().get(offset+id.value as usize).copied();
                    }
                    break;
                }
                offset+=registry.count;
            }
            let mut index=old.filter(|&i|i>0).ok_or_else(||
                <S::Error as serde::ser::Error>::custom("content ID is not in the authenticated V1 manifest"))? as usize;
            let mut ordinal=None;
            for (namespace,registry) in registries.iter().enumerate() {
                let count=registry["count"].as_u64().unwrap() as usize;
                if index<=count {ordinal=Some((namespace+1,index as u16));break;}
                index-=count;
            }
            wire.push(ordinal.ok_or_else(||<S::Error as serde::ser::Error>::custom("invalid V1 content ordinal"))?);
        }
        wire.serialize(s)
    }
    pub fn deserialize<'de,D:serde::Deserializer<'de>>(d:D)->Result<[ContentId;ID_LANES+2],D::Error> {
        let raw=Vec::<(usize,u16)>::deserialize(d)?;
        if raw.len()!=ID_LANES+2 {return Err(serde::de::Error::custom("invalid native ID lane count"));}
        let mut ids=[ContentId::ABSENT;ID_LANES+2];
        for (id,(namespace,value)) in ids.iter_mut().zip(raw) {
            if namespace==0 && value==0 {continue;}
            *id=(if RELATIONAL.get() {
                namespace.checked_sub(1).and_then(|n|REGISTRIES.get(n))
                    .and_then(|r|value.checked_sub(1).and_then(|v|ContentId::from_index(r.namespace,v as usize)))
            }else{
                let registries=v1_content_manifest()["registries"].as_array().unwrap();
                namespace.checked_sub(1).and_then(|n|registries.get(n).map(|r|(n,r)))
                    .filter(|(_,r)|value>0 && u64::from(value)<=r["count"].as_u64().unwrap())
                    .and_then(|(n,_)| {
                        let offset=registries[..n].iter().map(|r|r["count"].as_u64().unwrap() as usize).sum::<usize>();
                        manifest::v1_ids().get(offset+value as usize).copied()
                    })
            })
                .ok_or_else(||serde::de::Error::custom("invalid checkpoint content ID"))?;
        }
        Ok(ids)
    }
}

impl NativeDecision {
    pub fn trajectory_json(&self,relational:bool)->Result<String,serde_json::Error> {
        with_checkpoint_observation_mode(relational,|| {
            if relational {return serde_json::to_string(self);}
            // V1 hashes authenticate main's seven-field wire, not the V2-only packing metadata.
            #[derive(serde::Serialize)]
            struct Legacy<'a> {
                rows:&'a Vec<Features>,candidates:&'a Vec<Features>,
                sources:&'a Vec<usize>,targets:&'a Vec<usize>,slots:&'a Vec<i32>,
                #[serde(skip_serializing_if="Vec::is_empty")]
                actors:&'a Vec<u8>,
                #[serde(skip_serializing_if="Vec::is_empty")]
                owners:&'a Vec<u8>,
            }
            serde_json::to_string(&Legacy{rows:&self.rows,candidates:&self.candidates,
                sources:&self.sources,targets:&self.targets,slots:&self.slots,actors:&self.actors,owners:&self.owners})
        })
    }
    /// Exact original-layout inputs, never an old-manifest admission shortcut.
    pub fn legacy_projection(&self)->Option<&Self> {self.projection.as_deref()}
    pub(crate) fn legacy_member(&self,row:usize)->bool {
        self.legacy.get(row).copied().unwrap_or_else(||self.rows[row].legacy_row())
    }
    pub fn is_team(&self) -> bool { !self.actors.is_empty() }
    pub fn action_actor(&self, position: usize) -> Option<u8> {
        self.actors.get(position).and_then(|actor| actor.checked_sub(1))
    }
    pub fn action_slot(&self, position: usize) -> Option<usize> {
        self.slots.get(position).and_then(|slot| usize::try_from(*slot).ok())
    }
    pub fn layout(&self)->(usize,usize) { (self.rows.len(),self.slots.len()) }
    pub fn head(&self)->&'static str {
        if self.candidates.iter().any(|c|c.ids[0].value==ActionKind::Choose as u16
            || c.ids[0].value==ActionKind::ConfirmChoice as u16) {return "Choice";}
        let phase=self.rows[0].ids[2];
        phase.namespace.registry().and_then(|r|phase.value.checked_sub(1)
            .and_then(|i|r.names.get(i as usize))).copied().unwrap_or("Other")
    }
    pub fn action_name(&self,position:usize)->&'static str {
        let kind=self.candidates[position].ids[0];
        kind.namespace.registry().and_then(|r|kind.value.checked_sub(1)
            .and_then(|i|r.names.get(i as usize))).copied().unwrap_or("Other")
    }
    pub fn model_name(&self,position:usize)->&'static str {
        let model=self.candidates[position].ids[1];
        model.namespace.registry().and_then(|r|model.value.checked_sub(1)
            .and_then(|i|r.names.get(i as usize))).copied().unwrap_or("Absent")
    }
    /// Policy choices are monotone within a multi-select screen. Undo clicks
    /// edit UI state but add no final subset/order unreachable by selecting
    /// those cards directly. Keep original engine legality separately below.
    pub fn from_view(v: ObservationView<'_>) -> Result<Self, String> {
        Self::pack_view(v,true)
    }
    /// Full original support for legal-action parity; never apply policy-only
    /// cycle prevention to the oracle's legal-action comparison.
    pub fn from_original_view(v: ObservationView<'_>) -> Result<Self, String> {
        Self::pack_view(v,false)
    }
    /// State-only input for a critic target, never a policy decision.
    pub fn from_value_view(v:ObservationView<'_>)->Result<Self,String> {
        v.identity.validate().map_err(|e|format!("{e:?}"))?;
        if !matches!(v.status,DecisionStatus::Actionable|DecisionStatus::Waiting) {
            return Err("invalid critic observation status".into());
        }
        Self::pack_state(&v)
    }
    fn pack_view(v:ObservationView<'_>,monotone:bool)->Result<Self,String> {
        v.identity.validate().map_err(|e| format!("{e:?}"))?;
        if v.status != DecisionStatus::Actionable { return Err("not an actionable observation".into()); }
        let mut out=Self::pack_state(&v)?;
        Self::pack_actions(&v,monotone,&mut out);
        if out.slots.is_empty() {return Err("empty actionable observation".into());}
        if out.projection.as_ref().is_some_and(|old|old.slots!=out.slots) {
            return Err("legacy projection policy support differs from V2".into());
        }
        Ok(out)
    }
    fn pack_state(v:&ObservationView<'_>)->Result<Self,String> {
        let mut rows = Vec::with_capacity(v.state_len);
        for i in 0..v.state_len {
            let r = v.rows;
            let mut ids = [ContentId::ABSENT; ID_LANES + 2];
            ids[0] = ContentId { namespace: Namespace::RowKind, value: r.kind[i] as u16 };
            ids[1] = r.model[i];
            ids[2..].copy_from_slice(&r.detail[i]);
            rows.push(Features::new(ids, &r.integers[i], &r.floats[i],
                r.integer_present[i], r.float_present[i], r.flags[i], r.flags_present[i]));
        }
        if rows.is_empty() {return Err("empty critic state".into());}
        let mut owners = Vec::new();
        if v.scope == DecisionScope::TeamCombat {
            for i in 0..v.state_len {
                let mut owner = Some(i);
                let mut seat = 0;
                while let Some(row) = owner {
                    if v.rows.kind[row] == RowKind::Seat {
                        let absolute = v.rows.integers[row][0];
                        if !(0..MAX_SEATS as i32).contains(&absolute) { return Err("invalid team row owner".into()); }
                        seat = absolute as u8 + 1; break;
                    }
                    owner = v.rows.owner[row].index();
                }
                owners.push(seat);
            }
        }
        let mut relations = std::collections::BTreeSet::new();
        for row in 0..v.state_len {
            for (handle, kind) in [(v.rows.parent[row], Relation::Parent), (v.rows.owner[row], Relation::Owner)] {
                if handle != RowHandle::ABSENT {
                    relations.insert([row + 1, handle.0 as usize, kind.wire_id() as usize]);
                }
            }
        }
        for edge in v.edges {
            relations.insert([edge.from.0 as usize, edge.to.0 as usize, edge.relation.wire_id() as usize]);
        }
        if relations.iter().any(|&[from,to,kind]| from == 0 || to == 0 ||
            from > v.state_len || to > v.state_len || kind == 0 || kind > Relation::COUNT) {
            return Err("invalid policy relation".into());
        }
        let seat_for = |actor: u8| (0..v.state_len).find(|&i|
            v.rows.kind[i] == RowKind::Seat && v.rows.integer_present[i] & 1 != 0 &&
            v.rows.integers[i][0] == actor as i32).map_or(0, |i|i+1);
        let mut ratios = vec![[0.;4]; v.state_len+1];
        for i in 0..v.state_len {
            if v.rows.kind[i] != RowKind::Seat {continue;}
            for (out,num,den) in [(0,2,3),(2,5,6)] {
                let known=v.rows.integer_present[i] & ((1<<num)|(1<<den)) == ((1<<num)|(1<<den));
                if known && v.rows.integers[i][den] > 0 {
                    ratios[i+1][out] = v.rows.integers[i][num] as f32 / v.rows.integers[i][den] as f32;
                    ratios[i+1][out+1] = 1.;
                }
            }
        }
        let direct=DirectContext {
            global:(0..v.state_len).find(|&i|v.rows.kind[i]==RowKind::Global).map_or(0,|i|i+1),
            value_seat:seat_for(v.decision.actor),
            seats:Vec::new(), ratios,
        };
        #[cfg(not(feature="obs-v2"))]
        let legacy=v2::legacy_members(&rows,v.rows.parent);
        #[cfg(feature="obs-v2")]
        let legacy=rows.iter().zip(v.rows.legacy_role).map(|(row,role)|
            row.legacy_row() && *role==LegacyRowRole::Shared).collect();
        #[cfg(feature="obs-v2")]
        let projection=v.legacy.map(|old|Self::pack_state(&Self::projection_view(old,v.identity)))
            .transpose()?.map(Box::new);
        #[cfg(not(feature="obs-v2"))]
        let projection=None;
        #[cfg(feature="obs-v2")]
        let multiplicity={
            if v.rows.multiplicity.contains(&0) {return Err("zero row multiplicity".into());}
            v.rows.multiplicity.to_vec()
        };
        #[cfg(not(feature="obs-v2"))]
        let multiplicity=Vec::new();
        let mut out=Self { rows, candidates: vec![], sources: vec![], targets: vec![], slots: vec![], actors: vec![], owners,
            relations:relations.into_iter().collect(), direct, groups:v2::summary_groups(v), legacy, projection,
            multiplicity,pool_multiplicity:vec![],message_multiplicity:vec![],degree:vec![],totals:None,streamed:None };
        #[cfg(feature="obs-v2")]
        {
            out.exclude_choice_pile_details(v);
            streams::pack(&mut out,v)?;
        }
        Ok(out)
    }
    #[cfg(feature="obs-v2")]
    fn projection_view(old:LegacyObservationView<'_>,identity:ManifestIdentity)->ObservationView<'_> {
        ObservationView {decision:old.decision,scope:old.scope,identity,status:old.status,
            state_len:old.state_len,candidate_len:old.candidate_len,rows:old.rows,
            candidates:old.candidates,edges:old.edges,legacy:None,streamed_piles:Default::default()}
    }
    fn pack_actions(v:&ObservationView<'_>,monotone:bool,out:&mut Self) {
        for i in 0..v.candidate_len {
            if !v.candidates.sampleable(i, v.candidate_len) { continue; }
            if monotone && sts2_native_obs::original_fault_candidate(&v, i) { continue; }
            let c = v.candidates;
            if monotone && c.action[i]==ActionKind::Choose {
                let target=c.target[i].index().unwrap_or(v.state_len);
                if target<v.state_len && v.rows.kind[target]==RowKind::ChoiceOption {
                    let parent=v.rows.parent[target].index().unwrap_or(v.state_len);
                    let selected=KnownFlags{value:c.flags[i],present:c.flags_present[i]}.get(Flag::Selected);
                    if parent<v.state_len && v.rows.kind[parent]==RowKind::Choice &&
                        (KnownFlags{value:v.rows.flags[parent],present:v.rows.flags_present[parent]}).get(Flag::SingleSelect)==Some(false)
                        && selected==Some(true) {continue;}
                }
            }
            let mut ids = [ContentId::ABSENT; ID_LANES + 2];
            ids[0] = ContentId { namespace: Namespace::ActionKind, value: c.action[i] as u16 };
            ids[1] = c.model[i];
            ids[2..].copy_from_slice(&c.detail[i]);
            out.candidates.push(Features::new(ids, &c.integers[i], &c.floats[i],
                c.integer_present[i], c.float_present[i], c.flags[i], c.flags_present[i]));
            out.sources.push(c.source[i].0 as usize);
            out.targets.push(c.target[i].0 as usize);
            out.slots.push(i as i32);
            out.direct.seats.push((0..v.state_len).find(|&row|v.rows.kind[row]==RowKind::Seat &&
                v.rows.integer_present[row]&1!=0 && v.rows.integers[row][0]==c.actor[i] as i32).map_or(0,|row|row+1));
            if v.scope == DecisionScope::TeamCombat { out.actors.push(c.actor[i] + 1); }
        }
        #[cfg(feature="obs-v2")]
        if let Some(old)=v.legacy {
            Self::pack_actions(&Self::projection_view(old,v.identity),monotone,out.projection.as_mut().unwrap());
        }
    }
}

/// The encoder sees every row of the minibatch (~250k), so its weight gradient is
/// one GEMM with a tiny [in, hidden] output and a huge reduction: a few dozen thread
/// blocks run while the rest of the GPU idles. Splitting the rows into a broadcast
/// batch makes autodiff compute per-split partial gradients in parallel and sum them.
fn split_rows_linear(linear: &Linear, input: Tensor<2>) -> Tensor<2> {
    const MIN_SPLIT_ROWS: usize = 4096;
    let [rows, width] = input.dims();
    let mut splits = 1;
    while splits < 64 && rows % (splits * 2) == 0 && rows / (splits * 2) >= MIN_SPLIT_ROWS { splits *= 2; }
    if splits == 1 { return linear.forward(input); }
    let weight = linear.weight.val();
    let out = input.reshape([splits, rows / splits, width]).matmul(weight.unsqueeze_dim::<3>(0));
    let [_, _, hidden] = out.dims();
    let out = out.reshape([rows, hidden]);
    match &linear.bias { Some(bias) => out + bias.val().unsqueeze_dim::<2>(0), None => out }
}

#[derive(Module, Debug)]
pub struct NativePolicy {
    /// Registry-count-sized disjoint namespaces in one physical offset table.
    pub embedding: Embedding,
    encoder: Linear,
    actor_hidden: Linear,
    actor: Linear,
    critic: Linear,
    v2: Option<v2::RelationalPolicy>,
    pub hidden: usize,
}

impl NativePolicy {
    pub fn init_joint(hidden: usize, device: &Device) -> Self {
        let mut model = Self::init(hidden, device);
        model.actor_hidden = LinearConfig::new(hidden * 5, hidden).init(device);
        model
    }
    pub fn is_joint(&self) -> bool { self.actor_hidden.weight.val().dims()[0] == self.hidden * 5 }
    pub fn init(hidden: usize, device: &Device) -> Self {
        Self::init_layout(hidden,false,device)
    }
    fn init_layout(hidden:usize,relational:bool,device:&Device)->Self {
        assert!(hidden > 0);
        let width = crate::native_batch::embedding_width();
        Self {
            embedding: EmbeddingConfig::new(if relational {crate::native_batch::table_rows()}else{manifest::v1_table_rows()}, width).init(device),
            encoder: LinearConfig::new(width + NUMBERS, hidden).init(device),
            actor_hidden: LinearConfig::new(hidden * 4, hidden).init(device),
            actor: LinearConfig::new(hidden, 1).init(device),
            critic: LinearConfig::new(hidden, 1).init(device),
            v2: None,
            hidden,
        }
    }

    #[cfg(test)]
    fn encode(&self, rows: &[Features], device: &Device) -> Tensor<2> {
        let d = NativeDecision { rows:rows.to_vec(), candidates:vec![rows[0].clone()],
            sources:vec![0], targets:vec![0], slots:vec![0], actors:vec![], owners:vec![], relations:vec![],
            direct:DirectContext::default(), groups:vec![], legacy:vec![], projection:None,
            multiplicity:vec![],pool_multiplicity:vec![],message_multiplicity:vec![],degree:vec![],totals:None,streamed:None };
        let p = crate::native_batch::NativeHostBatch::pack(&[&d]).upload(device);
        self.encode_packed(&p).slice([0..rows.len()])
    }

    /// Power-of-two row/candidate buckets; masked output positions are not actions.
    pub fn forward(&self, decisions: &[&NativeDecision], device: &Device) -> PackedOut {
        self.forward_packed(&crate::native_batch::NativeHostBatch::pack(decisions).upload(device))
    }

    fn encode_packed(&self, p: &crate::native_batch::NativeBatch) -> Tensor<2> {
        use crate::native_batch::{INT_ROW, SLOTS};
        let n = p.r + p.k;
        let rows = p.ints.clone().slice([0..p.b, 0..n*INT_ROW]).reshape([p.b*n, INT_ROW]);
        let mut ids = rows.clone().slice([0..p.b*n, 0..SLOTS]).reshape([p.b*n*SLOTS, 1]);
        if !self.is_v2() {
            let device=ids.device();
            let lookup=manifest::v1_lookup();
            ids=Tensor::<1,Int>::from_data(TensorData::new(lookup.to_vec(),[lookup.len()]),&device)
                .gather(0,ids.reshape([p.b*n*SLOTS])).reshape([p.b*n*SLOTS,1]);
        }
        let width = crate::native_batch::embedding_width();
        let present = ids.clone().not_equal_elem(0).float();
        let embedded = (self.embedding.weight.val().gather(0, ids.repeat_dim(1, width)) * present)
            .reshape([p.b*n, SLOTS, width]).sum_dim(1).squeeze_dim::<2>(1);
        let raw = p.floats.clone().slice([0..p.b, 0..n*NUMERIC_LANES]).reshape([p.b*n, NUMERIC_LANES]);
        let mask = |column| rows.clone().slice([0..p.b*n, column..column+1])
            .bitwise_and(p.bits.clone()).not_equal_elem(0).float();
        let ip = mask(SLOTS);
        let fp = mask(SLOTS+1);
        let numeric = Tensor::cat(vec![
            (raw.clone()*ip.clone()).unsqueeze_dim::<3>(2),
            (raw*fp.clone()).unsqueeze_dim::<3>(2),
            ip.unsqueeze_dim::<3>(2), fp.unsqueeze_dim::<3>(2)], 2)
            .reshape([p.b*n, NUMERIC_LANES*4]);
        let flags = rows.slice([0..p.b*n, SLOTS+2..INT_ROW]);
        let flags = Tensor::cat(vec![flags.clone().equal_elem(2).float().unsqueeze_dim::<3>(2),
            flags.not_equal_elem(0).float().unsqueeze_dim::<3>(2)], 2).reshape([p.b*n, Flag::COUNT*2]);
        gelu(split_rows_linear(&self.encoder, Tensor::cat(vec![embedded,numeric,flags], 1)))
    }

    fn state_context(&self,p:&crate::native_batch::NativeBatch,state:Tensor<3>)->Tensor<3> {
        let n=p.r+p.k;
        let weights=p.floats.clone().slice([0..p.b,n*NUMERIC_LANES..n*NUMERIC_LANES+p.r])
            .reshape([p.b,p.r,1]);
        (state*weights).sum_dim(1)
    }

    /// Fit waiting-state values without a categorical distribution or policy
    /// gradients. In particular, zero-candidate observations need no fake action.
    pub fn calibrate_value_packed(&self,p:&crate::native_batch::NativeBatch)->Tensor<1> {
        // Do not build an encoder autodiff graph that critic-only backward
        // never visits; freezing the resulting context is too late.
        let frozen=self.valid();
        let all=frozen.encode_packed(p).reshape([p.b,p.r+p.k,self.hidden]);
        let state=all.slice([0..p.b,0..p.r,0..self.hidden]);
        let extra=frozen.stream_summaries(p,self.v2.is_some());
        let context=if let Some(old)=p.projection.as_deref() {
            let encoded=frozen.encode_packed(old).reshape([old.b,old.r+old.k,self.hidden]);
            frozen.state_context(old,encoded.slice([0..old.b,0..old.r,0..self.hidden]))
        }else{
            let mut context=frozen.state_context(p,state.clone());
            if let Some(extra)=&extra {context=context+extra.legacy.clone().reshape([p.b,1,self.hidden]);}
            context
        }.reshape([p.b,self.hidden]).set_require_grad(false);
        let mut value=self.critic.forward(context).reshape([p.b]);
        if let Some(v2)=&self.v2 {
            let state=Tensor::cat(vec![Tensor::<3>::zeros([p.b,1,self.hidden],&state.device()),state],1);
            let mut entities=frozen.v2.as_ref().unwrap().children.forward(p,state.clone())+state.clone();
            if let Some(extra)=&extra {entities=entities+extra.messages.clone();}
            let mut pools=v2.pools(p,entities);
            if let Some(extra)=&extra {pools=pools+extra.pools.clone();}
            value=value+v2.value(v2.value_features(p,state,pools).set_require_grad(false)).reshape([p.b]);
        }
        value
    }

    /// Raw candidate logits, including masked padding positions for diagnostics.
    pub fn logits_packed(&self, p: &crate::native_batch::NativeBatch) -> (Tensor<2>,Tensor<1>) {
        if self.v2.is_none() {
            let base=p.projection.as_deref().unwrap_or(p);
            let all=self.encode_packed(base).reshape([base.b,base.r+base.k,self.hidden]);
            let extra=self.stream_summaries(base,false);
            let (_,_,scores,value)=self.baseline_packed(base,all,extra.as_ref());
            return (scores,value);
        }
        let all=self.encode_packed(p).reshape([p.b,p.r+p.k,self.hidden]);
        let extra=self.stream_summaries(p,true);
        let (state,candidates,mut scores,mut value)=if let Some(old)=p.projection.as_deref() {
            let encoded=self.encode_packed(old).reshape([old.b,old.r+old.k,self.hidden]);
            let (_,_,scores,value)=self.baseline_packed(old,encoded,None);
            let state=Tensor::cat(vec![Tensor::<3>::zeros([p.b,1,self.hidden],&all.device()),
                all.clone().slice([0..p.b,0..p.r,0..self.hidden])],1);
            (state,all.slice([0..p.b,p.r..p.r+p.k,0..self.hidden]),scores,value)
        }else{self.baseline_packed(p,all,extra.as_ref())};
        if let Some(v2)=&self.v2 {
            let mut entities=v2.children.forward(p,state.clone())+state.clone();
            if let Some(extra)=&extra {entities=entities+extra.messages.clone();}
            let mut pools=v2.pools(p,entities.clone());
            if let Some(extra)=&extra {pools=pools+extra.pools.clone();}
            let features=v2.actor_features(p,candidates,state.clone(),entities,pools.clone());
            scores=scores+v2.actor.forward(gelu(v2.actor_hidden.forward(features))).reshape([p.b,p.k]);
            value=value+v2.value(v2.value_features(p,state,pools)).reshape([p.b]);
        }
        (scores,value)
    }
    fn baseline_packed(&self,p:&crate::native_batch::NativeBatch,all:Tensor<3>,extra:Option<&streams::StreamSummary>)
        ->(Tensor<3>,Tensor<3>,Tensor<2>,Tensor<1>) {
        use crate::native_batch::INT_ROW;
        let n = p.r+p.k;
        let h = self.hidden;
        let state = all.clone().slice([0..p.b, 0..p.r, 0..h]);
        let mut context = self.state_context(p,state.clone());
        if let Some(extra)=extra {context=context+extra.legacy.clone().reshape([p.b,1,h]);}
        let value = self.critic.forward(context.clone().reshape([p.b,h])).reshape([p.b]);
        assert!(!p.team || self.is_joint(), "team observations require a joint policy checkpoint");
        let owner_context = if self.is_joint() && p.team {
            let mut context = self.owner_summaries(p,state.clone());
            if let Some(extra)=extra {context=context+extra.owners.clone();}
            Some(Tensor::cat(vec![Tensor::<3>::zeros([p.b,1,h], &all.device()), context],1))
        } else { None };
        let state = Tensor::cat(vec![Tensor::<3>::zeros([p.b,1,h], &all.device()), state], 1);
        let index = |side| p.ints.clone().slice([0..p.b, n*INT_ROW+side*p.k..n*INT_ROW+(side+1)*p.k])
            .reshape([p.b,p.k,1]).repeat_dim(2,h);
        let candidates=all.slice([0..p.b,p.r..n,0..h]);
        let mut parts = vec![candidates.clone(),
            context.repeat_dim(1,p.k), state.clone().gather(1,index(0)),state.clone().gather(1,index(1))];
        if self.is_joint() {
            parts.push(match owner_context {
                Some(owner) => owner.gather(1,index(3)),
                None => Tensor::<3>::zeros([p.b,p.k,h], &p.ints.device()),
            });
        }
        let width = parts.len() * h;
        let features = Tensor::cat(parts, 2);
        let scores = self.actor.forward(gelu(self.actor_hidden.forward(features.reshape([p.b*p.k, width]))))
            .reshape([p.b,p.k]);
        (state,candidates,scores,value)
    }
    pub fn forward_packed(&self,p:&crate::native_batch::NativeBatch)->PackedOut {
        let (scores,value)=self.logits_packed(p);
        let n=p.r+p.k;
        let invalid=p.ints.clone().slice([0..p.b,n*crate::native_batch::INT_ROW+2*p.k..
            n*crate::native_batch::INT_ROW+3*p.k]).equal_elem(0);
        PackedOut { logp:masked_log_probs(scores,invalid), value }
    }
}

#[path="native_warm_start.rs"]
mod native_warm_start;
#[cfg(test)]
#[path="native_v2_tests.rs"]
mod v2_tests;
#[cfg(all(test,feature="obs-v2"))]
#[path="native_projection_tests.rs"]
mod projection_tests;
#[cfg(all(test,feature="obs-v2"))]
#[path="native_weight_tests.rs"]
mod weight_tests;

#[cfg(test)]
#[path="native_team_tests.rs"]
mod team_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_lookup_matches_dense_namespace_reference_with_duplicate_and_absent_ids() {
        let _rng_guard = crate::RNG_TEST_LOCK.lock().unwrap();
        let device = Device::flex();
        let model = NativePolicy::init(8, &device);
        let card = ContentId::from_index(Namespace::CardId, 0).unwrap();
        let relic = ContentId::from_index(Namespace::RelicId, 0).unwrap();
        let slots = ID_LANES + 2;
        let mut ids = vec![ContentId::ABSENT; slots];
        ids[0] = card;
        ids[1] = card;
        ids[2] = relic;
        let rows = vec![
            Features::new(ids, &[0; NUMERIC_LANES], &[0.; NUMERIC_LANES], 0, 0, 0, 0),
            Features::new(vec![ContentId::ABSENT; slots], &[0; NUMERIC_LANES], &[0.; NUMERIC_LANES], 1, 0, 0, 0),
        ];
        let weights = model.embedding.weight.val().into_data().try_to_vec::<f32>().unwrap();
        let width = crate::native_batch::embedding_width();
        let mut offsets = vec![];
        let mut offset = 0;
        for registry in REGISTRIES { offsets.push(offset); offset += registry.count; }
        let mut embedded = vec![0.;rows.len()*width];
        for (row,f) in rows.iter().enumerate() {
            for id in &f.ids {
                if id.value == 0 { continue; }
                let id = manifest::v1_lookup()[offsets[id.namespace as usize-1] + id.value as usize] as usize;
                if id==0 {continue;}
                for column in 0..width { embedded[row*width+column] += weights[id*width+column]; }
            }
        }
        let mut parts = vec![Tensor::<2>::from_data(TensorData::new(embedded,[2,width]),&device)];
        parts.push(Tensor::<2>::from_data(TensorData::new(
            rows.iter().flat_map(|r| r.numeric_channels()).collect::<Vec<_>>(), [2, NUMBERS]), &device));
        let expected = gelu(model.encoder.forward(Tensor::cat(parts, 1))).into_data().try_to_vec::<f32>().unwrap();
        let actual = model.encode(&rows, &device).into_data().try_to_vec::<f32>().unwrap();
        assert!(actual.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5));
    }
    #[test]
    fn native_numeric_presence_distinguishes_unknown_zero_integer_and_fraction() {
        let integers = [0; NUMERIC_LANES];
        let mut floats = [0.; NUMERIC_LANES];
        floats[0] = -0.375;
        let unknown = Features::new(vec![], &integers, &[0.; NUMERIC_LANES], 0, 0, 0, 0);
        let zero = Features::new(vec![], &integers, &[0.; NUMERIC_LANES], 1, 0, 0, 0);
        let fractional = Features::new(vec![], &integers, &floats, 0, 1, 0, 0);
        assert_eq!(&unknown.numeric_channels()[..4], &[0., 0., 0., 0.]);
        assert_eq!(&zero.numeric_channels()[..4], &[0., 0., 1., 0.]);
        assert_eq!(&fractional.numeric_channels()[..4], &[0., scaled_number(-0.375), 0., 1.]);
    }

    #[cfg(feature="obs-v2")]
    #[test]
    fn plain_policy_encoder_is_single_pass_v1_and_relational_encoder_is_normal_v2() {
        use sts2_sim::run::{RunManager,RunOptions,mp::Character};
        let game=RunManager::new_for_character(400,RunOptions::default(),Character::Ironclad);
        let id=DecisionId::default();
        let (mut actual,mut expected,mut normal)=(Box::new(NativeObsV1::new()),
            Box::new(NativeObsV1::new()),Box::new(NativeObsV1::new()));
        encoder_for_policy(false).encode_solo(&game,id,&mut actual).unwrap();
        NativeEncoder::v1_compatibility().encode_solo(&game,id,&mut expected).unwrap();
        encoder_for_policy(true).encode_solo(&game,id,&mut normal).unwrap();
        let pack=|obs:&NativeObsV1|NativeDecision::from_view(obs.view().unwrap()).unwrap();
        let (a,b,c)=(pack(&actual),pack(&expected),pack(&normal));
        assert!(a.legacy_projection().is_none() && c.legacy_projection().is_none());
        assert_eq!(serde_json::to_value(&a).unwrap(),serde_json::to_value(&b).unwrap());
        assert_ne!(serde_json::to_value(&a).unwrap(),serde_json::to_value(&c).unwrap());
    }
    pub(super) fn public_observation(amount: f32) -> NativeDecision {
        let mut obs = Box::new(NativeObsV1::new());
        let decision = DecisionId::default();
        obs.clear(decision);
        obs.push_row(Row { kind:RowKind::Global, ..Row::default() }).unwrap();
        let mut modifier = Row { kind:RowKind::Modifier, ..Row::default() };
        modifier.numbers.set_float(row_fields::modifier::AMOUNT,amount).unwrap();
        obs.push_row(modifier).unwrap();
        for i in 0..4 {
            let command = Command::MpPlayCard { hand:i, target:None };
            obs.push_candidate(Candidate { action:command.action(), present:true, legal:i!=1,
                model:ContentId::from_index(Namespace::CardId,i as usize).unwrap(),
                ..Candidate::default() }, Binding { decision,command,source_instance:Some(i),..Binding::default() }).unwrap();
        }
        obs.finish(DecisionStatus::Actionable).unwrap();
        NativeDecision::from_view(obs.view().unwrap()).unwrap()
    }

    fn multi_select(selected:&[usize])->Box<NativeObsV1> {
        let mut obs=Box::new(NativeObsV1::new());
        let decision=DecisionId::default();obs.clear(decision);
        obs.push_row(Row{kind:RowKind::Global,..Row::default()}).unwrap();
        let mut choice=Row{kind:RowKind::Choice,..Row::default()};
        choice.flags.set(Flag::SingleSelect,false);
        choice.numbers.set_integer(1,3).unwrap();choice.numbers.set_integer(2,3).unwrap();
        choice.numbers.set_integer(3,selected.len() as i32).unwrap();
        let parent=obs.push_row(choice).unwrap();
        for index in 0..4 {
            let mut row=Row{kind:RowKind::ChoiceOption,parent,..Row::default()};
            row.flags.set(Flag::Selected,selected.contains(&index));
            row.numbers.set_integer(0,index as i32).unwrap();
            let target=obs.push_row(row).unwrap();
            let command=Command::Solo(SoloCommand::Choose{index:index as u32});
            obs.push_candidate(Candidate{action:ActionKind::Choose,source:target,target,
                flags:row.flags,present:true,legal:true,..Candidate::default()},
                Binding{decision,command,..Binding::default()}).unwrap();
        }
        obs.finish(DecisionStatus::Actionable).unwrap();obs
    }

    #[test]
    fn dredge_seed400_slot3_cannot_be_toggled_by_policy_but_remains_originally_legal() {
        let obs=multi_select(&[3]);
        let original=NativeDecision::from_original_view(obs.view().unwrap()).unwrap();
        let policy=NativeDecision::from_view(obs.view().unwrap()).unwrap();
        assert_eq!(original.slots,vec![0,1,2,3]);
        assert_eq!(policy.slots,vec![0,1,2]);
        assert!(obs.view().unwrap().candidates.sampleable(3,4));
    }

    #[test]
    fn monotone_multi_select_preserves_every_fixed_count_subset_and_order() {
        let mut paths=vec![vec![]];
        for _ in 0..3 {
            paths=paths.into_iter().flat_map(|prefix| {
                let obs=multi_select(&prefix);
                NativeDecision::from_view(obs.view().unwrap()).unwrap().slots.into_iter().map(move|slot|{
                    let mut next=prefix.clone();next.push(slot as usize);next
                })
            }).collect();
        }
        let orders:std::collections::BTreeSet<_>=paths.into_iter().collect();
        assert_eq!(orders.len(),4*3*2);
        for a in 0..4 {for b in 0..4 {for c in 0..4 {
            if a!=b && b!=c && a!=c {assert!(orders.contains(&vec![a,b,c]));}
        }}}
    }

    #[test]
    fn original_throwing_pet_action_is_not_a_policy_outcome() {
        let mut obs = Box::new(NativeObsV1::new());
        let decision = DecisionId::default();
        obs.clear(decision);
        obs.push_row(Row { kind: RowKind::Global, ..Row::default() }).unwrap();
        let mut pet = Row { kind: RowKind::Creature, ..Row::default() };
        pet.flags.set(Flag::Pet, true);
        let pet = obs.push_row(pet).unwrap();
        let mut player = Row { kind: RowKind::Creature, ..Row::default() };
        player.flags.set(Flag::Pet, false);
        let player = obs.push_row(player).unwrap();
        let ignition = Namespace::CardId.registry().unwrap().names.iter()
            .position(|name| *name == "Ignition").unwrap();
        let model = ContentId::from_index(Namespace::CardId, ignition).unwrap();
        for target in [pet, player] {
            let command = Command::MpPlayCard { hand: 0, target: Some(0) };
            obs.push_candidate(Candidate { action: ActionKind::PlayCard, model, target,
                present: true, legal: true, ..Candidate::default() },
                Binding { decision, command, source_instance: Some(0), ..Binding::default() }).unwrap();
        }
        obs.finish(DecisionStatus::Actionable).unwrap();
        assert_eq!(NativeDecision::from_original_view(obs.view().unwrap()).unwrap().slots, vec![0, 1]);
        assert_eq!(NativeDecision::from_view(obs.view().unwrap()).unwrap().slots, vec![1]);
    }

    #[test]
    fn split_rows_linear_matches_one_gemm_output_and_parameter_gradients() {
        // Random init/inputs draw from the shared Flex RNG that seeded tests replay.
        let _rng_guard = crate::RNG_TEST_LOCK.lock().unwrap();
        let device=Device::flex().autodiff();
        let linear=LinearConfig::new(24,16).init(&device);
        let rows=4096*8;
        let input=Tensor::<2>::random([rows,24],burn::tensor::Distribution::Normal(0.,1.),&device);
        let target=Tensor::<2>::random([rows,16],burn::tensor::Distribution::Normal(0.,1.),&device);
        let run=|split:bool|{
            let out=if split {split_rows_linear(&linear,input.clone())} else {linear.forward(input.clone())};
            let loss=(out.clone()*target.clone()).sum();
            let grads=loss.backward();
            [out.detach().into_data().try_to_vec::<f32>().unwrap(),
             linear.weight.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap(),
             linear.bias.as_ref().unwrap().grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap()]
        };
        for (one,split) in run(false).iter().zip(run(true).iter()) {
            assert_eq!(one.len(),split.len());
            for (a,b) in one.iter().zip(split) {assert!((a-b).abs()<=1e-3*a.abs().max(1.),"{a} vs {b}");}
        }
    }
    #[test]
    fn padded_minibatch_matches_unpadded_loss_and_has_zero_padding_gradients() {
        let device=Device::flex().autodiff();
        let scores=Tensor::<2>::from_data(TensorData::new(vec![1.,-1.,-3.,3.],[2,2]),&device).require_grad();
        let value=Tensor::<1>::from_data(TensorData::new(vec![2.,9999.],[2]),&device).require_grad();
        let out=PackedOut{logp:masked_log_probs(scores.clone(),Tensor::<2,Bool>::from_data(
            TensorData::new(vec![false;4],[2,2]),&device)),value:value.clone()};
        let actions=Tensor::<2,Int>::zeros([2,1],&device);
        let old=Tensor::<1>::zeros([2],&device);
        let adv=Tensor::<1>::from_data(TensorData::new(vec![1.,50.],[2]),&device);
        let ret=Tensor::<1>::from_data(TensorData::new(vec![1.,-1000.],[2]),&device);
        let expected=crate::packed::ppo_loss(&PackedOut{logp:out.logp.clone().slice([0..1]),value:value.clone().slice([0..1])},
            actions.clone().slice([0..1]),old.clone().slice([0..1]),adv.clone().slice([0..1]),
            ret.clone().slice([0..1]),0.2,0.01,0.5).0.into_scalar::<f32>();
        let loss=crate::packed::ppo_loss_weighted(&out,actions,old,adv,ret,0.2,0.01,0.5,
            Tensor::<1>::from_data(TensorData::new(vec![1.,0.],[2]),&device)).0;
        assert!((loss.clone().into_scalar::<f32>()-expected).abs()<1e-6);
        let grads=loss.backward();
        let score_grad=scores.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap();
        assert_eq!(&score_grad[2..],&[0.,0.]);
        assert_eq!(value.grad(&grads).unwrap().into_data().try_to_vec::<f32>().unwrap()[1],0.);
    }
    #[test]
    fn masked_entropy_bonus_matches_analytic_gradient_through_multi_select_support() {
        check_masked_entropy_bonus(Device::flex().autodiff());
    }
    fn check_masked_entropy_bonus(device:Device) {
        let values=vec![2f32,-1.,0.5,30.];
        for mask in [[false,false,false,true],[false,true,false,true],[false,true,true,true]] {
            let scores=Tensor::<2>::from_data(TensorData::new(values.clone(),[1,4]),&device).require_grad();
            let logp=masked_log_probs(scores.clone(),Tensor::<2,Bool>::from_data(
                TensorData::new(mask.to_vec(),[1,4]),&device));
            let denominator=values.iter().zip(mask).filter(|(_,masked)|!*masked)
                .map(|(value,_)|(*value as f64).exp()).sum::<f64>();
            let p:Vec<_>=values.iter().zip(mask).map(|(value,masked)|
                if masked {0.}else{(*value as f64).exp()/denominator}).collect();
            let expected_h=-p.iter().filter(|&&p|p>0.).map(|p|p*p.ln()).sum::<f64>();
            let out=PackedOut{logp,value:Tensor::<1>::zeros([1],&device)};
            let (loss,_,entropy)=crate::packed::ppo_loss(&out,
                Tensor::<2,Int>::zeros([1,1],&device),Tensor::<1>::zeros([1],&device),
                Tensor::<1>::zeros([1],&device),Tensor::<1>::zeros([1],&device),0.2,1.,0.);
            assert!((entropy.into_scalar::<f32>() as f64-expected_h).abs()<1e-5);
            assert!((loss.clone().into_scalar::<f32>() as f64+expected_h).abs()<1e-5);
            let gradient=scores.grad(&loss.backward()).unwrap().into_data().try_to_vec::<f32>().unwrap();
            for (actual,p) in gradient.iter().zip(p) {
                let expected=if p==0. {0.}else{p*(p.ln()+expected_h)};
                assert!((*actual as f64-expected).abs()<1e-5);
            }
        }
    }
    #[test]
    fn padding_cannot_win_below_negative_billion_and_has_zero_ppo_gradient() {
        let device = Device::flex().autodiff();
        let scores = Tensor::<2>::from_data(TensorData::new(
            vec![-1e12,-1e12,-1e12,1e12], [1,4]),&device).require_grad();
        let invalid = Tensor::<2,Bool>::from_data(
            TensorData::new(vec![false,false,false,true],[1,4]),&device);
        let logp = masked_log_probs(scores.clone(),invalid);
        let p = logp.clone().exp().into_data().try_to_vec::<f32>().unwrap();
        assert!(p[..3].iter().all(|p|(*p-1./3.).abs()<1e-6));
        assert_eq!(p[3],0.);
        let out = PackedOut { logp,value:Tensor::<1>::zeros([1],&device) };
        let (loss,_,entropy) = crate::packed::ppo_loss(&out,
            Tensor::<2,Int>::zeros([1,1],&device),Tensor::<1>::full([1],-3f32.ln(),&device),
            Tensor::<1>::ones([1],&device),Tensor::<1>::zeros([1],&device),0.2,0.01,0.5);
        assert!(loss.clone().into_data().try_to_vec::<f32>().unwrap()[0].is_finite());
        assert!((entropy.into_data().try_to_vec::<f32>().unwrap()[0]-3f32.ln()).abs()<1e-6);
        let gradients = scores.grad(&loss.backward()).unwrap().into_data().try_to_vec::<f32>().unwrap();
        assert!(gradients.iter().all(|v|v.is_finite()));
        assert!(gradients[..3].iter().any(|v|v.abs()>0.));
        assert_eq!(gradients[3],0.);
    }

    #[test]
    fn extreme_public_numbers_keep_legal_distribution_and_updates_finite() {
        let _rng_guard = crate::RNG_TEST_LOCK.lock().unwrap();
        let device = Device::flex().autodiff();
        device.seed(1);
        let mut model = NativePolicy::init(64,&device);
        let decisions: Vec<_> = [0.,1e12,-1e12,f32::MAX,-f32::MAX].map(public_observation).into();
        let refs: Vec<_> = decisions.iter().collect();
        assert!(scaled_number(f32::MAX).is_finite());
        assert!(scaled_number(-f32::MAX).is_finite());
        assert_eq!(scaled_number(0.),0.);
        let out = model.forward(&refs,&device);
        let probabilities = out.logp.clone().exp().into_data().try_to_vec::<f32>().unwrap();
        for row in probabilities.chunks(4) {
            assert!(row.iter().all(|p|p.is_finite()));
            assert!((row[..3].iter().sum::<f32>()-1.).abs()<1e-6);
            assert_eq!(row[3],0.);
        }
        let actions = Tensor::<2,Int>::zeros([refs.len(),1],&device);
        let old = out.logp.clone().gather(1,actions.clone()).reshape([refs.len()]).detach();
        let (loss,_,entropy) = crate::packed::ppo_loss(&out,actions,old,
            Tensor::<1>::ones([refs.len()],&device),Tensor::<1>::zeros([refs.len()],&device),0.2,0.01,0.5);
        assert!(loss.clone().into_data().try_to_vec::<f32>().unwrap()[0].is_finite());
        assert!(entropy.into_data().try_to_vec::<f32>().unwrap().iter().all(|x|x.is_finite()));
        let gradients = burn::optim::GradientsParams::from_grads(loss.backward(),&model);
        model = burn::optim::AdamConfig::new().init().step(0.0003,model,gradients);
        assert!(model.forward(&refs,&device).value.into_data().try_to_vec::<f32>().unwrap()
            .iter().all(|x|x.is_finite()));
    }

    #[test]
    fn unreferenced_state_changes_candidate_preferences() {
        let _rng_guard = crate::RNG_TEST_LOCK.lock().unwrap();
        let device = Device::flex();
        device.seed(1);
        let model = NativePolicy::init(64,&device);
        let a = public_observation(0.);
        let b = public_observation(100.);
        // All sources/targets are absent, and candidate contents are identical
        // between decisions: only the unreferenced modifier changed.
        let probabilities = model.forward(&[&a,&b],&device).logp.exp().into_data().try_to_vec::<f32>().unwrap();
        assert!(probabilities[..3].iter().zip(&probabilities[4..7]).any(|(a,b)|(a-b).abs()>1e-6));
    }
}
