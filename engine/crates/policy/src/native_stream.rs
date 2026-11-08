use super::*;
use super::weights::PoolTotals;
use crate::native_batch::{NativeBatch,StreamBatch};
use burn::tensor::IndexingUpdateOp;

pub(crate) const CHUNK_ROWS:usize=512;

#[derive(Clone,Debug,Default,serde::Serialize,serde::Deserialize)]
pub(crate) struct StreamedFacts {
    pub(crate) rows:Vec<NativeDecision>,
    pub(crate) edges:Vec<StreamEdges>,
}
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub(crate) struct StreamEdges {
    pub(crate) input:NativeDecision,
    pub(crate) main:Vec<usize>,
}

pub(super) struct StreamSummary {
    pub(super) legacy:Tensor<2>,
    pub(super) owners:Tensor<3>,
    pub(super) pools:Tensor<3>,
    pub(super) messages:Tensor<3>,
}

impl NativeDecision {
    pub(crate) fn empty_stream_chunk(&self)->Self {
        Self {rows:vec![self.rows[0].clone()],candidates:vec![],sources:vec![],targets:vec![],slots:vec![],
            actors:vec![],owners:vec![],relations:vec![],direct:DirectContext::default(),groups:vec![0],
            legacy:vec![false],projection:None,multiplicity:vec![1],pool_multiplicity:vec![0],
            message_multiplicity:vec![0],degree:vec![],totals:Some(self.pool_totals()),streamed:None}
    }
}

#[cfg(feature="obs-v2")]
struct Fact {feature:Features,multiplicity:u32,owner:u8,group:u8,legacy:bool}

#[cfg(feature="obs-v2")]
fn feature(row:&Row)->Features {
    let mut ids=[ContentId::ABSENT;ID_LANES+2];
    ids[0]=ContentId {namespace:Namespace::RowKind,value:row.kind as u16};
    ids[1]=row.model;ids[2..].copy_from_slice(&row.detail);
    Features::new(ids,&row.numbers.integers,&row.numbers.floats,row.numbers.integer_present,
        row.numbers.float_present,row.flags.value,row.flags.present)
}

#[cfg(feature="obs-v2")]
fn chunk(root:&NativeDecision,facts:&[Fact],ids:&[usize],degree:&[u64],totals:PoolTotals,pointwise:bool)->NativeDecision {
    let n=root.rows.len();
    let mut out=root.empty_stream_chunk();
    out.rows.clear();out.groups.clear();out.legacy.clear();out.multiplicity.clear();
    out.pool_multiplicity.clear();out.message_multiplicity.clear();
    out.degree=vec![0];out.totals=Some(totals);
    for &id in ids {
        let (value,mass,owner,group,legacy)=if id<=n {
            (root.rows[id-1].clone(),root.row_multiplicity(id-1) as u32,
                root.owners.get(id-1).copied().unwrap_or(0),0,false)
        }else{
            let fact=&facts[id-n-1];
            (fact.feature.clone(),fact.multiplicity,fact.owner,fact.group,fact.legacy)
        };
        out.rows.push(value);out.multiplicity.push(mass);out.groups.push(group);out.legacy.push(legacy);
        if !root.owners.is_empty() {out.owners.push(owner);}
        out.pool_multiplicity.push(if pointwise {mass}else{0});
        out.message_multiplicity.push(if id>n {mass}else{0});
        out.degree.push(degree[id]);
    }
    out
}

#[cfg(feature="obs-v2")]
pub(super) fn pack(root:&mut NativeDecision,v:&ObservationView<'_>)->Result<(),String> {
    if v.streamed_piles.groups.is_empty() {return Ok(());}
    let n=root.rows.len();
    let rows=v.streamed_piles.weighted_rows().map(|(row,m)|(*row,m)).collect::<Vec<_>>();
    let mut facts=Vec::with_capacity(rows.len());
    let mut owners=Vec::with_capacity(rows.len());
    let mut edges=std::collections::BTreeSet::new();
    let mut offset=0;
    for group in v.streamed_piles.groups {
        let pile=group.pile.index().filter(|&i|i<n).ok_or("invalid streamed pile")?;
        let summary=root.groups[pile];
        if summary==0 {return Err("streamed pile has no summary group".into());}
        let resolve=|handle:RowHandle|->Result<usize,String> {
            if handle.0>PILE_LOCAL_BASE {
                let local=usize::from(handle.0-PILE_LOCAL_BASE);
                if local==0 || local>group.row_len {return Err("invalid stream-local handle".into());}
                Ok(n+offset+local)
            }else if usize::from(handle.0)<=n {Ok(usize::from(handle.0))}
            else {Err("invalid streamed main handle".into())}
        };
        for local in 0..group.row_len {
            let (row,mass)=rows[offset+local];
            if mass==0 || row.multiplicity!=mass {return Err("invalid streamed multiplicity".into());}
            let value=feature(&row);
            let legacy=row.legacy_role==LegacyRowRole::Shared && value.legacy_row();
            facts.push(Fact {feature:value,multiplicity:mass,owner:0,group:summary,legacy});
            owners.push(resolve(row.owner)?);
            let from=n+offset+local+1;
            for (to,kind) in [(row.parent,Relation::Parent),(row.owner,Relation::Owner)] {
                if to!=RowHandle::ABSENT {edges.insert([from,resolve(to)?,kind.wire_id() as usize]);}
            }
        }
        for edge in &v.streamed_piles.edges[group.edge_start..group.edge_start+group.edge_len] {
            let from=resolve(edge.from)?;let to=resolve(edge.to)?;
            if from==0 || to==0 {return Err("absent streamed edge endpoint".into());}
            edges.insert([from,to,edge.relation.wire_id() as usize]);
        }
        offset+=group.row_len;
    }
    if !root.owners.is_empty() {
        for index in 0..facts.len() {
            let mut owner=owners[index];
            let mut resolved=false;
            for _ in 0..=facts.len() {
                if owner<=n {
                    facts[index].owner=if owner==0 {0}else{root.owners[owner-1]};
                    resolved=true;break;
                }
                owner=owners[owner-n-1];
            }
            if !resolved {return Err("cyclic streamed owner".into());}
        }
    }
    let mut totals=root.pool_totals();
    for fact in &facts {totals.add(u64::from(fact.multiplicity),fact.legacy,usize::from(fact.owner),usize::from(fact.group));}
    let mut degree=Vec::new();root.fill_degree(&mut degree);degree.resize(n+facts.len()+1,0);
    let mass=|id:usize|if id<=n {root.row_multiplicity(id-1)}else{u64::from(facts[id-n-1].multiplicity)};
    for &[from,to,_] in &edges {
        let weight=mass(from).max(mass(to));degree[from]+=weight;degree[to]+=weight;
    }
    let mut output=StreamedFacts::default();
    let ids=(n+1..n+facts.len()+1).collect::<Vec<_>>();
    for block in ids.chunks(CHUNK_ROWS) {output.rows.push(chunk(root,&facts,block,&degree,totals,true));}
    let mut ids=Vec::new();
    let mut positions=std::collections::BTreeMap::new();
    let mut relations=Vec::new();
    let flush=|ids:&mut Vec<usize>,positions:&mut std::collections::BTreeMap<usize,usize>,
        relations:&mut Vec<[usize;3]>,output:&mut StreamedFacts| {
        if relations.is_empty() {return;}
        let mut input=chunk(root,&facts,ids,&degree,totals,false);
        input.relations=std::mem::take(relations);
        let main=ids.iter().map(|&id|if id<=n {id}else{0}).collect();
        output.edges.push(StreamEdges {input,main});ids.clear();positions.clear();
    };
    for [from,to,kind] in edges {
        let extra=usize::from(!positions.contains_key(&from))+usize::from(from!=to && !positions.contains_key(&to));
        if ids.len()+extra>CHUNK_ROWS {flush(&mut ids,&mut positions,&mut relations,&mut output);}
        let mut local=|id|*positions.entry(id).or_insert_with(|| {ids.push(id);ids.len()});
        relations.push([local(from),local(to),kind]);
    }
    flush(&mut ids,&mut positions,&mut relations,&mut output);
    root.degree=degree[..n+1].to_vec();root.totals=Some(totals);root.streamed=Some(Box::new(output));
    Ok(())
}

impl NativePolicy {
    pub(super) fn owner_summaries(&self,p:&NativeBatch,state:Tensor<3>)->Tensor<3> {
        let h=self.hidden;
        if !self.is_joint() || !p.team {return Tensor::zeros([p.b,MAX_SEATS,h],&state.device());}
        let start=(p.r+p.k)*NUMERIC_LANES+p.r;
        p.floats.clone().slice([0..p.b,start..start+MAX_SEATS*p.r])
            .reshape([p.b,MAX_SEATS,p.r]).matmul(state)
    }
    pub(super) fn stream_summaries(&self,p:&NativeBatch,relational:bool)->Option<StreamSummary> {
        if p.stream_rows.is_empty() && p.stream_edges.is_empty() {return None;}
        let h=self.hidden;let device=p.ints.device();
        let mut out=StreamSummary {legacy:Tensor::zeros([p.b,h],&device),
            owners:Tensor::zeros([p.b,MAX_SEATS,h],&device),pools:Tensor::zeros([p.b,1,3*h],&device),
            messages:Tensor::zeros([p.b,p.r+1,h],&device)};
        for input in &p.stream_rows {
            let all=self.encode_packed(input).reshape([p.b,input.r+input.k,h]);
            let state=all.slice([0..p.b,0..input.r,0..h]);
            out.legacy=out.legacy+self.state_context(input,state.clone()).reshape([p.b,h]);
            out.owners=out.owners+self.owner_summaries(input,state.clone());
            if relational {
                let state=Tensor::cat(vec![Tensor::zeros([p.b,1,h],&device),state],1);
                out.pools=out.pools+self.v2.as_ref().unwrap().pools(input,state);
            }
        }
        if relational {
            for StreamBatch {input,main} in &p.stream_edges {
                let all=self.encode_packed(input).reshape([p.b,input.r+input.k,h]);
                let state=Tensor::cat(vec![Tensor::zeros([p.b,1,h],&device),
                    all.slice([0..p.b,0..input.r,0..h])],1);
                let messages=self.v2.as_ref().unwrap().children.forward(input,state);
                out.pools=out.pools+self.v2.as_ref().unwrap().pools(input,messages.clone());
                let mask=main.clone().not_equal_elem(0).float().reshape([p.b,input.r+1,1]);
                let refs=main.clone().reshape([p.b,input.r+1,1]).repeat_dim(2,h);
                out.messages=out.messages.scatter(1,refs,messages*mask,IndexingUpdateOp::Add);
            }
        }
        Some(out)
    }
}
