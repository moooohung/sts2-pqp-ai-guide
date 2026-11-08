use super::*;
use crate::native_batch::NativeBatch;

impl Features {
    fn kind_name(&self)->&'static str {
        let id=self.ids[0];
        id.namespace.registry().and_then(|r|id.value.checked_sub(1).and_then(|i|r.names.get(i as usize)))
            .copied().unwrap_or("")
    }
    pub(crate) fn legacy_row(&self)->bool {
        self.ids[0].namespace!=Namespace::RowKind || matches!(self.kind_name(),
            "Global"|"Seat"|"Creature"|"Intent"|"Card"|"Power"|"Relic"|"Potion"|"Orb"|"Rune"|
            "Modifier"|"MapNode"|"MapEdge"|"Choice"|"ChoiceOption"|"Reward"|"Offer"|
            "Event"|"EventPage"|"EventOption"|"RestService"|"Treasure")
    }
}
pub(super) fn legacy_members(rows:&[Features],parents:&[RowHandle])->Vec<bool> {
    (0..rows.len()).map(|row| {
        let mut index=row;
        for _ in 0..rows.len() {
            if !rows[index].legacy_row() {return false;}
            match parents[index].index() {Some(parent)=>index=parent,None=>return true}
        }
        false
    }).collect()
}

/// V1-compatible projection of the V2 interface's public grouping metadata.
/// Unknown row kinds inherit their parent, never row adjacency or content IDs.
#[cfg(not(feature="obs-v2"))]
pub(super) fn summary_groups(v:&ObservationView<'_>)->Vec<u8> {
    (0..v.state_len).map(|row| {
        let mut current=Some(row);
        for _ in 0..v.state_len {
            let Some(i)=current else {break;};
            let name=Namespace::RowKind.registry().unwrap().names[v.rows.kind[i] as usize-1];
            let pile=||v.rows.detail[i].iter().find(|id|id.namespace==Namespace::PileKind)
                .and_then(|id|id.value.checked_sub(1)).and_then(|n|Namespace::PileKind.registry().unwrap().names.get(n as usize));
            match name {
                "Global"|"Seat"=>return 0,
                "Creature"|"Intent"|"Power"|"Orb"|"Rune"=>return 1,
                "Card"|"PileSummary"=>return if pile().is_some_and(|&name|name!="None") {1}else{2},
                "MapNode"|"MapEdge"|"RouteSummary"|"Boss"=>return 3,
                "Relic"|"Potion"|"Choice"|"ChoiceOption"|"Reward"|"Offer"|"Event"|
                "EventPage"|"EventOption"|"RestService"|"Treasure"|"UpgradePreview"=>return 2,
                _=>current=v.rows.parent[i].index(),
            }
        }
        0
    }).collect()
}
#[cfg(feature="obs-v2")]
pub(super) fn summary_groups(v:&ObservationView<'_>)->Vec<u8> {
    (0..v.state_len).map(|row|match v.summary_group(row) {
        None=>0,Some(SummaryGroup::Combat)=>1,Some(SummaryGroup::Deck)=>2,Some(SummaryGroup::Map)=>3,
    }).collect()
}

#[derive(Module,Debug)]
pub(super) struct RelationalPolicy {
    pub(super) children:relations::ChildAggregation,
    pub(super) actor_hidden:Linear,
    pub(super) actor:Linear,
    pub(super) critic_hidden:Linear,
    pub(super) critic:Linear,
}
impl RelationalPolicy {
    pub(super) fn init(h:usize,device:&Device)->Self {
        Self {children:relations::ChildAggregation::init(h,device),
            actor_hidden:LinearConfig::new(8*h+4,h).init(device),
            actor:LinearConfig::new(h,1).init(device),
            critic_hidden:LinearConfig::new(5*h+4,h).init(device),
            critic:LinearConfig::new(h,1).init(device)}
    }
    pub(super) fn value(&self,features:Tensor<2>)->Tensor<2> {
        self.critic.forward(gelu(self.critic_hidden.forward(features)))
    }
    fn direct(p:&NativeBatch,state:Tensor<3>,start:usize,count:usize)->(Tensor<3>,Tensor<3>) {
        let h=state.dims()[2];
        let refs=p.ints.clone().slice([0..p.b,start..start+count]).reshape([p.b,count,1]);
        let encoded=state.gather(1,refs.clone().repeat_dim(2,h));
        let ratios=p.floats.clone().slice([0..p.b,p.ratio_f()..p.ratio_f()+(p.r+1)*4])
            .reshape([p.b,p.r+1,4]).gather(1,refs.repeat_dim(2,4));
        (encoded,ratios)
    }
    pub(super) fn pools(&self,p:&NativeBatch,state:Tensor<3>)->Tensor<3> {
        let h=state.dims()[2];
        let weights=p.floats.clone().slice([0..p.b,p.pool_f()..p.pool_f()+3*p.r]).reshape([p.b,3,p.r]);
        weights.matmul(state.slice([0..p.b,1..p.r+1,0..h])).reshape([p.b,1,3*h])
    }
    pub(super) fn actor_features(&self,p:&NativeBatch,candidates:Tensor<3>,state:Tensor<3>,entities:Tensor<3>,pools:Tensor<3>)->Tensor<2> {
        let h=state.dims()[2];
        let (seat,ratios)=Self::direct(p,state.clone(),p.direct_i()+2,p.k);
        let (global,_)=Self::direct(p,state,p.direct_i(),1);
        let refs=|side|p.ints.clone().slice([0..p.b,(p.r+p.k)*crate::native_batch::INT_ROW+side*p.k..
            (p.r+p.k)*crate::native_batch::INT_ROW+(side+1)*p.k]).reshape([p.b,p.k,1]).repeat_dim(2,h);
        Tensor::cat(vec![candidates,entities.clone().gather(1,refs(0)),entities.gather(1,refs(1)),
            seat,global.repeat_dim(1,p.k),pools.repeat_dim(1,p.k),ratios],2)
            .reshape([p.b*p.k,8*h+4])
    }
    pub(super) fn value_features(&self,p:&NativeBatch,state:Tensor<3>,pools:Tensor<3>)->Tensor<2> {
        let h=state.dims()[2];
        let (seat,ratios)=Self::direct(p,state.clone(),p.direct_i()+1,1);
        let (global,_)=Self::direct(p,state,p.direct_i(),1);
        Tensor::cat(vec![seat,global,pools,ratios],2)
            .reshape([p.b,5*h+4])
    }
}
impl NativePolicy {
    /// Explicit new lineage; `init` remains the authenticated legacy loader/baseline.
    pub fn init_v2(hidden:usize,device:&Device)->Self {
        let mut model=Self::init_layout(hidden,true,device);
        model.v2=Some(RelationalPolicy::init(hidden,device));
        model
    }
    pub fn init_joint_v2(hidden:usize,device:&Device)->Self {
        let mut model=Self::init_v2(hidden,device);
        model.actor_hidden=LinearConfig::new(hidden*5,hidden).init(device);
        model.v2=Some(RelationalPolicy::init(hidden,device));
        model
    }
    pub fn is_v2(&self)->bool {self.v2.is_some()}
}
