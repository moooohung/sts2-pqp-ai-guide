//! Source-format normalization for the reference's public, unordered pile groups.
use super::*;

type RowKey=(u16,[(u16,u16);ID_LANES],u64,u64,u16,[i32;NUMERIC_LANES],u16,[u32;NUMERIC_LANES]);
#[derive(PartialEq,Eq,PartialOrd,Ord)]
struct Key {
    row:RowKey,
    enchantments:Vec<(u16,i32)>,
    variables:Vec<(u16,i32)>,
    tags:u64,
    affliction:Option<(u16,i32)>,
    previews:Vec<(String,i32,i32)>,
    extra:Vec<String>,
}
struct Group { indices:Vec<usize>,key:Key }
pub(super) struct Layout {
    pub order:Vec<usize>,
    pub aliases:Vec<usize>,
    pub multiplicity:Vec<u32>,
    pub deferred:Vec<bool>,
}
fn row_key(row:&Row)->RowKey{
    (row.model.value,row.detail.map(|id|(id.namespace as u16,id.value)),
        row.flags.present,row.flags.value,row.numbers.integer_present,row.numbers.integers,
        row.numbers.float_present,row.numbers.floats.map(f32::to_bits))
}
fn key(snapshot:&Snapshot,indices:&[usize])->Result<Key,String>{
    let source=&snapshot.rows[indices[0]];
    let root=learned_row(source,RowKind::Card)?;
    let mut key=Key{row:row_key(&root),enchantments:vec![],variables:vec![],tags:0,
        affliction:None,previews:vec![],extra:vec![]};
    if root.model==ContentId::ABSENT{
        key.extra.push(format!("{:?}",source.color.as_ref().map(|color|identity(Some(color)))));
    }
    for &index in &indices[1..]{
        let source=&snapshot.rows[index];
        let kind=RowKind::ALL.into_iter().find(|kind|format!("{kind:?}")==source.kind)
            .ok_or("unknown live pile child")?;
        let row=learned_row(source,kind)?;
        let amount=row.numbers.integers[0];
        if kind==RowKind::Modifier{
            match row.model.namespace{
                Namespace::EnchantmentId=>key.enchantments.push((row.model.value,amount)),
                Namespace::CardAffliction=>key.affliction=Some((row.model.value,amount)),
                Namespace::CardTag if row.model.value>0=>key.tags|=1<<(row.model.value-1),
                _=>if row.detail[0].namespace==Namespace::VarKey{
                    key.variables.push((row.detail[0].value,amount));
                },
            }
        }
        if kind==RowKind::CardPreview{
            let variable=row.detail[0];
            let name=variable.namespace.registry().and_then(|r|variable.value.checked_sub(1)
                .and_then(|i|r.names.get(usize::from(i)))).copied().unwrap_or("");
            key.previews.push((name.to_owned(),row.numbers.integers[0],row.numbers.integers[1]));
        }
        key.extra.push(format!("{:?}:{:?}",kind,row_key(&row)));
    }
    Ok(key)
}
pub(super) fn normalize(snapshot:&Snapshot,order:Vec<usize>,combat:bool)->Result<Layout,String>{
    let count=snapshot.rows.len();
    let mut layout=Layout{order,aliases:(0..count).collect(),
        multiplicity:vec![1;count],deferred:vec![false;count]};
    if !combat{return Ok(layout);}
    let targeted=layout.order.iter().copied().filter(|index|snapshot.rows[*index].kind=="CardPreview"&&
        snapshot.edges.iter().any(|(from,_,relation)|usize::from(*from)==*index+1&&relation=="Target"))
        .collect::<Vec<_>>();
    layout.order.retain(|index|!targeted.contains(index));
    let map=layout.order.iter().position(|index|snapshot.rows[*index].kind=="MapNode").unwrap_or(layout.order.len());
    layout.order.splice(map..map,targeted);
    let mut piles=BTreeMap::<usize,Vec<Group>>::new();
    for &index in &layout.order{
        let source=&snapshot.rows[index];
        let unordered=source.kind=="Card"&&source.owner==source.parent&&
            source.detail.get(3).and_then(Option::as_ref).is_some_and(|pile|
                pile.namespace=="PileKind"&&matches!(pile.name.as_str(),"Draw"|"Discard"|"Exhaust"|"Play"));
        if !unordered{continue;}
        let summary=snapshot.edges.iter().find_map(|(from,to,relation)|{
            if usize::from(*to)!=index+1||relation!="Member"{return None;}
            let from=usize::from(*from).checked_sub(1)?;
            (snapshot.rows.get(from)?.kind=="PileSummary").then_some(from)
        }).ok_or("live pile summary missing")?;
        let indices=layout.order.iter().copied().filter(|child|*child==index||
            usize::from(snapshot.rows[*child].parent)==index+1).collect::<Vec<_>>();
        let key=key(snapshot,&indices)?;
        for &child in &indices{layout.deferred[child]=true;}
        piles.entry(summary).or_default().push(Group{indices,key});
    }
    layout.order.retain(|index|!layout.deferred[*index]);
    for groups in piles.values_mut(){
        groups.sort_by(|a,b|a.key.cmp(&b.key));
        let mut start=0;
        while start<groups.len(){
            let mut end=start+1;
            while end<groups.len()&&groups[start].key==groups[end].key{end+=1;}
            let first=&groups[start].indices;
            for &index in first{layout.multiplicity[index]=(end-start) as u32;}
            for group in &groups[start+1..end]{
                for (&index,&alias)in group.indices.iter().zip(first){layout.aliases[index]=alias;}
            }
            layout.order.extend(first);
            start=end;
        }
    }
    Ok(layout)
}
