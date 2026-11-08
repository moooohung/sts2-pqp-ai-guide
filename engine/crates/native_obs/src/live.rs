//! Advice-only source adapter. Names and properties come from the live game;
//! learned rows, bounds, numeric validation and feature packing remain shared.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path="live_piles.rs"]
mod piles;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub namespace: String,
    pub name: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub kind: String,
    pub model: Option<Identity>,
    pub color: Option<Identity>,
    #[serde(default)]
    pub detail: Vec<Option<Identity>>,
    #[serde(default)]
    pub owner: u16,
    #[serde(default)]
    pub parent: u16,
    #[serde(default)]
    pub numbers: BTreeMap<String, i32>,
    #[serde(default)]
    pub floats: BTreeMap<String, f32>,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSnapshot {
    pub binding: String,
    pub action: String,
    pub source: u16,
    pub target: u16,
    pub model: Option<Identity>,
    #[serde(default="legal_default")]
    pub legal: bool,
    #[serde(default)]
    pub numbers: BTreeMap<String, i32>,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
}
fn legal_default()->bool{true}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub seat: u8,
    pub rows: Vec<Element>,
    pub options: Vec<OptionSnapshot>,
    #[serde(default)]
    pub edges: Vec<(u16, u16, String)>,
}

fn normalized(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c|c.to_ascii_uppercase()).collect()
}
fn original_card(name:&str)->Option<ContentId>{
    static ENTRIES:std::sync::OnceLock<BTreeMap<&'static str,ContentId>>=std::sync::OnceLock::new();
    ENTRIES.get_or_init(||sts2_sim::tables::CardId::ALL.into_iter().filter_map(|id|{
        let info=sts2_sim::tables::card_info(id,false)?;
        Some((info.entry,ContentId::from_index(Namespace::CardId,id.index())?))
    }).collect()).get(name).copied()
}
/// Unknown content is the existing ABSENT embedding, not a random embedding or
/// a nearest known model. Its captured numeric/categorical properties survive.
pub fn identity(value: Option<&Identity>) -> ContentId {
    let Some(value) = value else { return ContentId::ABSENT };
    // Resolution is a pure function of the name; a capture repeats the same few hundred
    // names on every request, and a registry miss normalizes every registry entry.
    thread_local!{static KNOWN:std::cell::RefCell<std::collections::HashMap<String,std::collections::HashMap<String,ContentId>>>=Default::default();}
    if let Some(id)=KNOWN.with_borrow(|known|known.get(&value.namespace).and_then(|names|names.get(&value.name)).copied()) {
        return id;
    }
    let id=resolve_identity(value);
    KNOWN.with_borrow_mut(|known|known.entry(value.namespace.clone()).or_default().insert(value.name.clone(),id));
    id
}
fn resolve_identity(value:&Identity)->ContentId {
    let original=match value.namespace.as_str() {
        "CardId"=>original_card(&value.name).or_else(||sts2_sim::tables::CardId::from_name(&value.name)
            .and_then(|id|ContentId::from_index(Namespace::CardId,id.index()))),
        "RelicId"=>sts2_sim::tables::RelicId::from_name(&value.name)
            .and_then(|id|ContentId::from_index(Namespace::RelicId,id.index())),
        "PotionId"=>sts2_sim::tables::PotionId::from_name(&value.name)
            .and_then(|id|ContentId::from_index(Namespace::PotionId,id.index())),
        "PowerId"=>sts2_sim::tables::PowerId::from_name(&value.name)
            .and_then(|id|ContentId::from_index(Namespace::PowerId,id.index())),
        "MonsterKind"=>sts2_sim::tables::MonsterKind::from_name(&value.name)
            .and_then(|id|ContentId::from_index(Namespace::MonsterKind,id.index())),
        "EventId"=>EventId::from_name(&value.name).map(Into::into),
        "EventPageId"=>EventPageId::from_name(&value.name).map(Into::into),
        "EventOptionId"=>EventOptionId::from_name(&value.name).map(Into::into),
        "PublicBossId"=>PublicBossId::from_name(&value.name).map(Into::into),
        _=>None,
    };
    if let Some(id)=original {return id;}
    let Some(registry) = REGISTRIES.iter().find(|r|r.name == value.namespace) else {
        return ContentId::ABSENT;
    };
    let mut wanted = normalized(&value.name);
    let suffix=match value.namespace.as_str() {
        "RelicPool"=>"RELICPOOL","OrbType"=>"ORB","PowerId"=>"POWER",_=>""
    };
    if !suffix.is_empty() && let Some(prefix)=wanted.strip_suffix(suffix){wanted=prefix.to_owned();}
    if value.namespace=="VarKey" {
        if wanted=="CALCULATIONBASE" {wanted="CALCBASE".into();}
        if !registry.names.iter().any(|name|normalized(name)==wanted) &&
            let Some(prefix)=wanted.strip_suffix("POWER") && !prefix.is_empty() {
            wanted=prefix.to_owned();
        }
    }
    registry.names.iter().position(|name|normalized(name)==wanted)
        .and_then(|index|ContentId::from_index(registry.namespace,index)).unwrap_or(ContentId::ABSENT)
}
fn numbers(element: &Element, schema: &[NumericField]) -> Result<Numbers,String> {
    let mut result=Numbers::EMPTY;
    for (name,value) in &element.numbers {
        let lane=schema.iter().position(|f|f.name==name).ok_or_else(||format!("unknown numeric field {name}"))?;
        result.set_integer(lane,*value).map_err(|e|format!("{e:?}"))?;
    }
    for (name,value) in &element.floats {
        let lane=schema.iter().position(|f|f.name==name).ok_or_else(||format!("unknown float field {name}"))?;
        if element.numbers.contains_key(name){return Err("overlapping numeric fields".into());}
        result.set_float(lane,*value).map_err(|e|format!("{e:?}"))?;
    }
    Ok(result)
}
/// Wire names are the variants' Debug names; index them once instead of formatting
/// every variant for every lookup.
fn named<T:Copy+std::fmt::Debug>(all:impl IntoIterator<Item=T>)->std::collections::HashMap<String,T>{
    all.into_iter().map(|value|(format!("{value:?}"),value)).collect()
}
fn flag_named(name:&str)->Option<Flag>{
    static NAMES:std::sync::OnceLock<std::collections::HashMap<String,Flag>>=std::sync::OnceLock::new();
    NAMES.get_or_init(||named(Flag::ALL)).get(name).copied()
}
fn row_kind_named(name:&str)->Option<RowKind>{
    static NAMES:std::sync::OnceLock<std::collections::HashMap<String,RowKind>>=std::sync::OnceLock::new();
    NAMES.get_or_init(||named(RowKind::ALL)).get(name).copied()
}
fn relation_named(name:&str)->Option<Relation>{
    static NAMES:std::sync::OnceLock<std::collections::HashMap<String,Relation>>=std::sync::OnceLock::new();
    NAMES.get_or_init(||named(Relation::ALL)).get(name).copied()
}
fn action_named(name:&str)->Option<ActionKind>{
    static NAMES:std::sync::OnceLock<std::collections::HashMap<String,ActionKind>>=std::sync::OnceLock::new();
    NAMES.get_or_init(||named(ActionKind::ALL)).get(name).copied()
}
fn flags(values:&BTreeMap<String,bool>)->Result<KnownFlags,String>{
    let mut result=KnownFlags::default();
    for (name,value) in values {
        let flag=flag_named(name).ok_or_else(||format!("unknown flag {name}"))?;
        result.set(flag,*value);
    }
    Ok(result)
}
fn learned_row(source:&Element,kind:RowKind)->Result<Row,String>{
    let mut row=Row{kind,model:identity(source.model.as_ref()),
        numbers:numbers(source,ROW_FIELDS[kind as usize])?,
        flags:flags(&source.flags)?,..Row::default()};
    for(lane,value)in source.detail.iter().enumerate(){row.detail[lane]=identity(value.as_ref());}
    if kind==RowKind::Power && row.model.namespace==Namespace::PowerId &&
        let Some(index)=row.model.value.checked_sub(1) &&
        let Some(&power)=sts2_sim::tables::PowerId::ALL.get(usize::from(index)){
        let meta=sts2_sim::tables::power_meta(power);
        row.detail[0]=ContentId::from_index(Namespace::PowerType,meta.power_type as usize).unwrap();
        row.detail[1]=ContentId::from_index(Namespace::PowerStackType,meta.stack_type as usize).unwrap();
    }
    // Same rule as powers above: the policy was trained on the native table's pool for each relic.
    // The game's own lookup returns the first pool listing a relic, which is Event for the few
    // relics that both the Event and the Shared pool list (Razor Tooth, Lasting Candy, Sparkling Rouge).
    if kind==RowKind::Relic && row.model.namespace==Namespace::RelicId &&
        let Some(index)=row.model.value.checked_sub(1) &&
        let Some(&relic)=sts2_sim::tables::RelicId::ALL.get(usize::from(index)){
        row.detail[1]=ContentId::from_index(Namespace::RelicPool,
            sts2_sim::tables::RELIC_INFO[relic.index()].pool as usize).unwrap();
    }
    Ok(row)
}
impl Snapshot {
    fn inventory_seat(&self,index:usize)->Option<usize>{
        let mut root=index;
        loop {
            let parent=usize::from(self.rows[root].parent).checked_sub(1)?;
            if self.rows[parent].kind=="Seat" {
                let row=&self.rows[root];
                let owned=match row.kind.as_str(){
                    "PileSummary"=>row.detail.first().and_then(Option::as_ref)
                        .is_some_and(|id|id.namespace=="PileKind"&&id.name=="None"),
                    "Relic"=>row.numbers.contains_key("inventory_order"),
                    "Potion"=>row.numbers.contains_key("slot"),
                    "Card"=>self.edges.iter().any(|(from,to,relation)|*to as usize==root+1&&relation=="Member"&&
                        self.rows.get(usize::from(*from).wrapping_sub(1)).is_some_and(|summary|
                            summary.kind=="PileSummary"&&summary.detail.first().and_then(Option::as_ref)
                                .is_some_and(|id|id.namespace=="PileKind"&&id.name=="None"))),
                    _=>false,
                };
                return owned.then_some(parent);
            }
            root=parent;
        }
    }
    fn variable_position(&self,source:&Element)->Option<usize>{
        let parent=self.rows.get(usize::from(source.parent).checked_sub(1)?)?;
        let card=identity(parent.model.as_ref());
        if card.namespace!=Namespace::CardId{return Some(usize::MAX);}
        let id=*sts2_sim::tables::CardId::ALL.get(usize::from(card.value).checked_sub(1)?)?;
        let upgraded=parent.numbers.get("upgrades").copied().unwrap_or(0)>0;
        let info=sts2_sim::tables::card_info(id,upgraded)?;
        let variable=identity(source.detail.first()?.as_ref());
        info.vars.iter().position(|(key,_)|usize::from(variable.value)==key.index()+1 &&
            variable.namespace==Namespace::VarKey)
    }
    pub fn encode(&self)->Result<Box<NativeObsV2>,String>{
        if self.schema!="sts2-live-advice-v1" || usize::from(self.seat)>=MAX_SEATS ||
            self.rows.len()>STATE_CAPACITY || self.options.len()>CANDIDATE_CAPACITY ||
            self.edges.len()>EDGE_CAPACITY {return Err("invalid live snapshot envelope".into());}
        let mut obs=Box::new(NativeObsV2::new());
        obs.clear(DecisionId{actor:self.seat,..DecisionId::default()});
        let mut handles=vec![RowHandle::ABSENT;self.rows.len()+1];
        for(index,source)in self.rows.iter().enumerate(){
            if source.detail.len()>ID_LANES{return Err("too many content lanes".into());}
            if usize::from(source.owner)>index||usize::from(source.parent)>index{
                return Err("invalid live parent or owner".into());
            }
        }
        let retained=|row:&Element|row.kind=="Modifier" && row.model.is_none() &&
            row.detail.first().and_then(Option::as_ref).is_some_and(|id|id.namespace=="VarKey");
        let mut order=(0..self.rows.len()).collect::<Vec<_>>();
        let mut start=0;
        while start<order.len(){
            if !retained(&self.rows[start]){start+=1;continue;}
            let mut end=start+1;
            while end<order.len() && retained(&self.rows[end]) &&
                self.rows[end].parent==self.rows[start].parent{end+=1;}
            // Factory metadata supplies only the retained key order; every amount is live data.
            order[start..end].sort_by_key(|index|self.variable_position(&self.rows[*index]));
            start=end;
        }
        let combat=self.rows.first().and_then(|row|row.detail.first()).and_then(Option::as_ref)
            .is_some_and(|phase|phase.name=="Combat");
        if !combat && self.rows.iter().filter(|row|row.kind=="Seat").count()>1{
            let mut sorted=Vec::with_capacity(order.len());
            let mut emitted=vec![false;order.len()];
            for &index in &order{
                if self.rows[index].kind=="Global"{sorted.push(index);emitted[index]=true;}
            }
            for (seat,row)in self.rows.iter().enumerate(){
                if row.kind!="Seat"{continue;}
                sorted.push(seat);emitted[seat]=true;
                for &index in &order{
                    if self.inventory_seat(index)==Some(seat){sorted.push(index);emitted[index]=true;}
                }
            }
            sorted.extend(order.into_iter().filter(|index|!emitted[*index]));
            order=sorted;
        }
        order.retain(|index|!retained(&self.rows[*index])||self.variable_position(&self.rows[*index]).is_some());
        let layout=piles::normalize(self,order,combat)?;
        for &index in &layout.order {
            let source=&self.rows[index];
            let kind=row_kind_named(&source.kind).ok_or("unknown live row kind")?;
            let mut row=learned_row(source,kind)?;
            row.owner=handles[usize::from(source.owner)];row.parent=handles[usize::from(source.parent)];
            row.multiplicity=layout.multiplicity[index];
            if kind==RowKind::Relic && !source.numbers.contains_key("inventory_order"){
                row.legacy_role=LegacyRowRole::V2Only;
            }
            // The native encoder lists a merchant's potion only in the relational observation;
            // potion rewards and the potion belt are visible to both.
            if kind==RowKind::Potion && usize::from(source.parent).checked_sub(1)
                .is_some_and(|parent|self.rows[parent].kind=="Offer"){
                row.legacy_role=LegacyRowRole::V2Only;
            }
            if layout.deferred[index]&&matches!(kind,RowKind::CardPreview|RowKind::EffectParameter){
                row.legacy_role=LegacyRowRole::V2Only;
            }
            let handle=obs.push_row(row).map_err(|e|format!("{e:?}"))?;
            handles[index+1]=handle;
            for (target,relation) in [(row.parent,Relation::Parent),(row.owner,Relation::Owner)]{
                if target!=RowHandle::ABSENT {
                    obs.push_edge(Edge{from:handle,to:target,relation}).map_err(|e|format!("{e:?}"))?;
                }
            }
            if kind==RowKind::Card && row.model==ContentId::ABSENT && source.color.is_some() {
                let mut color=Row{kind:RowKind::Modifier,model:identity(source.color.as_ref()),
                    parent:handle,owner:row.owner,multiplicity:row.multiplicity,..Row::default()};
                color.numbers.set_integer(0,0).map_err(|e|format!("{e:?}"))?;
                let child=obs.push_row(color).map_err(|e|format!("{e:?}"))?;
                for (target,relation) in [(handle,Relation::Parent),(row.owner,Relation::Owner)]{
                    if target!=RowHandle::ABSENT {obs.push_edge(Edge{from:child,to:target,relation})
                        .map_err(|e|format!("{e:?}"))?;}
                }
            }
        }
        for(index,alias)in layout.aliases.iter().copied().enumerate(){
            if alias!=index{handles[index+1]=handles[alias+1];}
        }
        let mut emitted=std::collections::BTreeSet::new();
        for (from,to,name) in &self.edges {
            let relation=relation_named(name).ok_or("unknown live relation")?;
            let from=*handles.get(usize::from(*from)).ok_or("invalid live edge")?;
            let to=*handles.get(usize::from(*to)).ok_or("invalid live edge")?;
            if !emitted.insert((from.0,to.0,relation.wire_id())){continue;}
            obs.push_edge(Edge{from,to,relation}).map_err(|e|format!("{e:?}"))?;
        }
        let mut bindings=std::collections::HashSet::new();
        for option in &self.options {
            if !bindings.insert(&option.binding){return Err("duplicate live binding".into());}
            let action=action_named(&option.action).ok_or("unknown live action")?;
            let source=Element{numbers:option.numbers.clone(),..Element::default()};
            let candidate=Candidate{action,actor:self.seat,model:identity(option.model.as_ref()),
                source:*handles.get(usize::from(option.source)).ok_or("invalid live source")?,
                target:*handles.get(usize::from(option.target)).ok_or("invalid live target")?,
                numbers:numbers(&source,CANDIDATE_FIELDS)?,flags:flags(&option.flags)?,
                present:true,legal:option.legal,..Candidate::default()};
            obs.push_advisor_candidate(candidate).map_err(|e|format!("{e:?}"))?;
        }
        obs.finish(if self.options.iter().any(|option|option.legal){DecisionStatus::Actionable}else{DecisionStatus::Waiting})
            .map_err(|e|format!("{e:?}"))?;
        Ok(obs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masked_live_actions_keep_original_slot_positions(){
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
            "schema":"sts2-live-advice-v1","seat":0,
            "rows":[{"kind":"Global"},{"kind":"Seat","parent":1}],
            "options":[
                {"binding":"first","action":"PlayCard","source":2,"target":0,"model":null},
                {"binding":"blocked","action":"PlayCard","source":2,"target":0,"model":null,"legal":false},
                {"binding":"end","action":"EndTurn","source":2,"target":0,"model":null,"legal":true}
            ]
        })).unwrap();
        let obs=snapshot.encode().unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.candidate_len,3);
        let sampled=(0..view.candidate_len).filter(|&i|
            view.candidates.sampleable(i,view.candidate_len)).collect::<Vec<_>>();
        assert_eq!(sampled,vec![0,2]);
    }
    #[test]
    fn live_power_stack_classification_matches_reference_metadata(){
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
            "schema":"sts2-live-advice-v1","seat":0,"rows":[
                {"kind":"Global"},{"kind":"Seat","parent":1},
                {"kind":"Creature","owner":2,"parent":1},
                {"kind":"Power","model":{"namespace":"PowerId","name":"SHRINK_POWER"},
                    "parent":3,"owner":3,"numbers":{"amount":-1,"display_counter":-1},
                    "detail":[{"namespace":"PowerType","name":"Debuff"},
                        {"namespace":"PowerStackType","name":"Single"}]}
            ],"options":[{"binding":"end","action":"EndTurn","source":2,"target":0,"model":null}]
        })).unwrap();
        let obs=snapshot.encode().unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.rows.detail[3][1],ContentId::from_index(Namespace::PowerStackType,0).unwrap());
        assert_eq!(view.rows.integers[3][0],-1);
        assert_eq!(view.rows.integers[3][1],-1);
    }
    #[test]
    fn offered_relics_do_not_enter_the_owned_inventory_projection(){
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
            "schema":"sts2-live-advice-v1","seat":0,"rows":[
                {"kind":"Global"},{"kind":"Seat","parent":1},
                {"kind":"Relic","owner":2,"parent":2,"numbers":{"inventory_order":0}},
                {"kind":"EventOption","owner":2},
                {"kind":"Relic","owner":2,"parent":4}
            ],"options":[{"binding":"offer","action":"EventChoice","source":4,"target":0,"model":null}]
        })).unwrap();
        let obs=snapshot.encode().unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.rows.legacy_role[2],LegacyRowRole::Shared);
        assert_eq!(view.rows.legacy_role[4],LegacyRowRole::V2Only);
        assert_eq!(view.rows.parent[4].0,4);
    }
    #[test]
    fn a_relic_in_two_game_pools_is_encoded_with_the_pool_the_policy_was_trained_on(){
        let pool_of=|relic:&str,pool:&str|{
            let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
                "schema":"sts2-live-advice-v1","seat":0,"rows":[
                    {"kind":"Global"},{"kind":"Seat","parent":1},
                    {"kind":"Offer","owner":2},
                    {"kind":"Relic","model":{"namespace":"RelicId","name":relic},"owner":2,"parent":3,
                        "detail":[{"namespace":"RelicRarity","name":"Rare"},{"namespace":"RelicPool","name":pool}]}
                ],"options":[{"binding":"buy","action":"BuyRelic","source":3,"target":0,"model":null}]
            })).unwrap();
            let obs=snapshot.encode().unwrap();
            obs.view().unwrap().rows.detail[3][1]
        };
        let shared=ContentId::from_index(Namespace::RelicPool,sts2_sim::tables::RELIC_INFO[
            sts2_sim::tables::RelicId::RazorTooth.index()].pool as usize).unwrap();
        // ModelDb.AllRelicPools lists Event before Shared, so the game reports EVENT_RELIC_POOL here.
        assert_eq!(pool_of("RAZOR_TOOTH","EVENT_RELIC_POOL"),shared);
        assert_eq!(pool_of("RAZOR_TOOTH","SHARED_RELIC_POOL"),shared);
        // A relic the game's tables do not know keeps the pool the live game reported.
        assert_ne!(pool_of("MOD_RELIC","EVENT_RELIC_POOL"),shared);
    }
    #[test]
    fn a_merchants_potion_is_relational_only_but_owned_and_reward_potions_are_shared(){
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
            "schema":"sts2-live-advice-v1","seat":0,"rows":[
                {"kind":"Global"},{"kind":"Seat","parent":1},
                {"kind":"Potion","owner":2,"parent":2,"numbers":{"slot":0,"capacity":2}},
                {"kind":"Offer","owner":2,"numbers":{"price":50,"offer_order":0}},
                {"kind":"Potion","owner":2,"parent":4,"numbers":{"slot":0,"capacity":0}},
                {"kind":"Reward","owner":2},
                {"kind":"Potion","owner":2,"parent":6,"numbers":{"slot":0,"capacity":2}}
            ],"options":[{"binding":"buy","action":"BuyPotion","source":4,"target":0,"model":null},
                {"binding":"take","action":"PickPotion","source":6,"target":0,"model":null}]
        })).unwrap();
        let obs=snapshot.encode().unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.rows.legacy_role[2],LegacyRowRole::Shared);
        assert_eq!(view.rows.legacy_role[4],LegacyRowRole::V2Only);
        assert_eq!(view.rows.legacy_role[6],LegacyRowRole::Shared);
    }
    #[test]
    fn unknown_content_preserves_properties_without_dispatch_authority(){
        assert_eq!(identity(Some(&Identity{namespace:"PowerId".into(),name:"STRENGTH_POWER".into()})),
            ContentId::from_index(Namespace::PowerId,sts2_sim::tables::PowerId::Strength.index()).unwrap());
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
            "schema":"sts2-live-advice-v1","seat":0,"rows":[
                {"kind":"Global","model":null},
                {"kind":"Seat","model":null,"parent":1,
                    "detail":[{"namespace":"Character","name":"TheQueen"}],"numbers":{"hp":45}},
                {"kind":"Card","model":{"namespace":"CardId","name":"MOD_INJECTED"},
                    "owner":2,"parent":2,"numbers":{"energy_cost":2,"display_damage":17},
                    "detail":[{"namespace":"CardType","name":"Attack"}]}
            ],"options":[{"binding":"mod","action":"PickCard","source":3,"target":0,"model":null}]
        })).unwrap();
        let obs=snapshot.encode().unwrap();
        let view=obs.view().unwrap();
        assert_eq!(view.rows.detail[1][0],ContentId::ABSENT);
        assert_eq!(view.rows.model[2],ContentId::ABSENT);
        assert_eq!(view.rows.integers[2][0],2);
        assert_eq!(view.rows.integers[2][5],17);
        assert_eq!(view.rows.detail[2][0],ContentId::from_index(Namespace::CardType,
            sts2_sim::tables::CardType::Attack as usize).unwrap());
        assert!(obs.binding(0,view.decision,1).is_err());
        assert_eq!(snapshot.encode().unwrap().view().unwrap().rows.integers[2],view.rows.integers[2]);
    }
}
