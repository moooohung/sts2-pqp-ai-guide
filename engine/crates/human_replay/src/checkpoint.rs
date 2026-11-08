//! Original checkpoint schema projection, outside the learned observation.
//! Unsupported models are exclusions, never copied from an expected snapshot.
use serde_json::{Value,json};
use sts2_sim::{card::Card, combat::Combat, creature::Creature, relic::Relic,
    rng::Rng,run::{replay_view::*,NativeEventOffer,NativeShopKind},
    tables::{RelicId,PotionId,POTION_INFO}};
use crate::folding::CardIdentity;

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Mismatch { pub code:&'static str, pub path:String }
impl Mismatch {
    pub fn unsupported(path:impl Into<String>)->Self {
        Self{code:"CHECKPOINT_PROJECTION_UNSUPPORTED",path:path.into()}
    }
}
pub type Result<T>=std::result::Result<T,Mismatch>;
pub fn rng(r:&Rng)->Value {json!({"counter":r.counter,"state":r.inner.s})}
pub fn card(c:&Card)->Result<Value> {
    let id=CardIdentity::from_sim(c).map_err(|_|Mismatch::unsupported("card identity"))?;
    Ok(json!({"id":id.id,"upgrade":id.upgrade}))
}
fn cards<'a>(items:impl IntoIterator<Item=&'a Card>)->Result<Value> {
    Ok(Value::Array(items.into_iter().map(card).collect::<Result<_>>()?))
}
fn creature(c:&Creature,id:&str,combat:&Combat)->Value {
    let powers:Vec<_>=c.power_list().iter().map(|&p|{
        let p=&combat.powers[p as usize];
        let name=p.id.name();
        json!({"id":if name.ends_with("_POWER"){name.to_owned()}else{format!("{name}_POWER")},
            "amount":p.amount})
    }).collect();
    let mut v=json!({"id":id,"hp":c.hp,"max_hp":c.max_hp,"block":c.block,"powers":powers});
    if let Some(ai)=&c.ai {v["rng"]=rng(&ai.rng);}
    v
}
fn relic(r:&Relic)->Result<Value> {
    // CanonicalVars and DisplayAmount from original Models/Relics/*.cs,
    // game 0861bfa1. All these models have base StackCount=1 and Normal status
    // except VenerableTeaSet's explicit saved activation flag.
    let vars=match r.id {
        RelicId::BurningBlood=>json!({"Heal":6}),
        RelicId::BoundPhylactery=>json!({"Summon":1}),
        RelicId::RingOfTheSnake=>json!({"Cards":2}),
        RelicId::CrackedCore=>json!({"Lightning":1}),
        RelicId::SmallCapsule|RelicId::ScrollBoxes=>json!({}),
        RelicId::StoneHumidifier=>json!({"MaxHp":5}),
        RelicId::VenerableTeaSet=>json!({"Energy":2}),
        RelicId::GoldenPearl=>json!({"Gold":150}),
        RelicId::FishingRod=>json!({"Combats":3}),
        RelicId::Lantern=>json!({"Energy":1}),
        _=>return Err(Mismatch::unsupported(format!("relic {}",r.id.name()))),
    };
    Ok(json!({"id":r.id.name(),"status":if r.id==RelicId::VenerableTeaSet&&r.flag1 {"Active"}else{"Normal"},
        "stacks":1,"display":if r.id==RelicId::FishingRod {r.counter%3}else{0},"vars":vars}))
}
pub(crate) fn potion_name(id:PotionId)->String {
    // Original Helpers/StringHelper.cs CamelCaseRegex + Slugify, applied to
    // the catalog's C# type identifier, not a translated/display name.
    let name=POTION_INFO[id.index()].id;
    let mut out=String::new();let mut previous=false;
    for c in name.trim().chars() {
        if c.is_ascii_uppercase()&&previous {out.push('_');}
        if c.is_ascii_alphanumeric()||c=='_' {out.push(c.to_ascii_uppercase());}
        previous=c.is_ascii_alphanumeric();
    }
    out
}
pub(crate) fn potion(id:PotionId)->Result<Value> {
    let name=potion_name(id);
    let vars=match name.as_str() {
        "BLOCK_POTION"=>json!({"Block":12}),
        "POWER_POTION"=>json!({}),
        "GIGANTIFICATION_POTION"=>json!({"GigantificationPower":1}),
        _=>return Err(Mismatch::unsupported(format!("potion {name}"))),
    };
    Ok(json!({"id":name,"queued":false,"vars":vars}))
}
fn bag(pools:&[Vec<RelicId>;4])->Value {
    let mut out=serde_json::Map::new();
    for (name,pool) in ["Common","Uncommon","Rare","Shop"].into_iter().zip(pools) {
        out.insert(name.into(),json!(pool.iter().map(|r|r.name()).collect::<Vec<_>>()));
    }
    Value::Object(out)
}
/// `reward_history` must be independently constructed from native offers and
/// folded selections. It is never the original checkpoint's rewards array.
pub fn project(v:&ReplayCheckpointView<'_>,reward_history:&[Value],local_seat:usize)->Result<Value> {
    if local_seat>=v.seats.len() {return Err(Mismatch::unsupported("local seat"));}
    let r=&v.run.rngs;
    let mut run_rng=serde_json::Map::new();
    for (name,value) in [("UpFront",&r.up_front),("Shuffle",&r.shuffle),
        ("UnknownMapPoint",&r.unknown_map_point),("CombatCardGeneration",&r.combat_card_generation),
        ("CombatPotionGeneration",&r.combat_potion_generation),("CombatCardSelection",&r.combat_card_selection),
        ("CombatEnergyCosts",&r.combat_energy_costs),("CombatTargets",&r.combat_targets),
        ("MonsterAi",&r.monster_ai),("Niche",&r.niche),("CombatOrbGeneration",&r.combat_orbs),
        ("TreasureRoomRelics",&r.treasure_room)] {run_rng.insert(name.into(),rng(value));}
    let mut players=Vec::new();
    for (seat,p) in v.seats.iter().enumerate() {
        let c=v.player_combat;
        let piles=if let Some(c)=c {
            let player=&c.players[seat as u8];
            [player.hand.as_slice(),player.draw.as_slice(),player.discard.as_slice(),player.exhaust.as_slice()]
                .into_iter().map(|pile|cards(pile.iter().map(|&k|&c.cards[k as usize])))
                .collect::<Result<Vec<_>>>()?
        }else{vec![json!([]);4]};
        let cr=if let Some(c)=c {
            let mut value=creature(c.cr(seat as u8),p.character.name(),c);
            // Persistent HP/max HP can change during post-combat rewards while
            // the original room retains its completed combat piles/powers.
            value["hp"]=p.hp.into();value["max_hp"]=p.max_hp.into();value
        }else{json!({"id":p.character.name(),"hp":p.hp,"max_hp":p.max_hp,"block":0,"powers":[]})};
        let potions=(0..p.potion_slots).map(|slot|match p.potions.get(slot).copied().flatten() {
            Some(id)=>potion(id),None=>Ok(Value::Null),
        }).collect::<Result<Vec<_>>>()?;
        players.push(json!({"seat":seat,
            "energy":c.map(|c|c.players[seat as u8].energy),"stars":c.map(|c|c.players[seat as u8].stars),
            "hand":piles[0],"draw":piles[1],"discard":piles[2],"exhaust":piles[3],"creature":cr,
            "rng":{"Rewards":rng(p.rng.rewards),"Shops":rng(p.rng.shops),"Transformations":rng(p.rng.transformations)},
            "relics":p.relics.iter().map(|r|relic(r)).collect::<Result<Vec<_>>>()?,
            "potions":potions,"card_rarity_odds":p.card_rarity_odds,"potion_reward_odds":p.potion_reward_odds,
            "relic_bag":bag(p.relic_bag)}));
    }
    let enemies:Vec<_>=v.combat.map(|c|c.enemies.iter().map(|&who|
        creature(c.cr(who),c.cr(who).monster_id,c)).collect()).unwrap_or_default();
    let mut next=Vec::new();
    if let Some(map)=v.map && let Some((x,y))=v.run.coord &&
        let Some(point)=map.point(sts2_sim::run::Coord::new(x,y)) {
        next=point.children.iter().map(|&i|{
            let p=map.points[i as usize].coord;vec![p.col,p.row]
        }).collect();next.sort_by_key(|p|p[0]);
    }
    let event_options=v.event.as_ref().map(|e|e.options.iter().map(|o|->Result<Value>{
        let relic=match o.offer {Some(NativeEventOffer::Relic(r))=>Some(r.name()),_=>None};
        let id=if e.id==sts2_sim::identity::EventId::Neow {
            format!("NEOW.pages.INITIAL.options.{}",relic.expect("Neow offers a relic"))
        }else{
            // Original Models/Events/SelfHelpBook.cs:53-78,
            // MorphicGrove.cs:33-34, Wellspring.cs:25-26. Typed registry
            // names are not the C# localization keys in checkpoint-v1.
            use sts2_sim::identity::EventOptionId as O;
            let key=match o.identity.id {
                O::SelfHelpBookInitialReadTheBack=>"SELF_HELP_BOOK.pages.INITIAL.options.READ_THE_BACK",
                O::SelfHelpBookInitialReadPassage=>"SELF_HELP_BOOK.pages.INITIAL.options.READ_PASSAGE",
                O::SelfHelpBookInitialReadEntireBook=>"SELF_HELP_BOOK.pages.INITIAL.options.READ_ENTIRE_BOOK",
                O::SelfHelpBookInitialNoOptions=>"SELF_HELP_BOOK.pages.INITIAL.options.NO_OPTIONS",
                O::MorphicGroveInitialGroup=>"MORPHIC_GROVE.pages.INITIAL.options.GROUP",
                O::MorphicGroveInitialLoner=>"MORPHIC_GROVE.pages.INITIAL.options.LONER",
                O::WellspringInitialBottle=>"WELLSPRING.pages.INITIAL.options.BOTTLE",
                O::WellspringInitialBathe=>"WELLSPRING.pages.INITIAL.options.BATHE",
                _=>return Err(Mismatch::unsupported("event option localization identity")),
            };
            if e.id==sts2_sim::identity::EventId::SelfHelpBook&&!o.enabled {
                format!("{key}_LOCKED")
            }else{key.to_owned()}
        };
        Ok(json!({"id":id,"enabled":o.enabled,"relic":relic}))
    }).collect::<Result<Vec<_>>>()).transpose()?;
    let shop=if v.shops.is_empty(){None}else{Some(v.shops[local_seat].iter().map(|o|{
        let (kind,model)=match o.kind {
            NativeShopKind::Card=>("MerchantCardEntry",o.card.map(|c|card(c)).transpose()?.map(|c|c["id"].clone())),
            NativeShopKind::Relic=>("MerchantRelicEntry",o.relic.map(|r|json!(r.name()))),
            NativeShopKind::Potion=>("MerchantPotionEntry",o.potion.map(potion).transpose()?.map(|p|p["id"].clone())),
            NativeShopKind::Removal=>("MerchantCardRemovalEntry",None),
        };
        Ok(json!({"kind":kind,"model":model,"price":o.price,"stocked":!o.sold}))
    }).collect::<Result<Vec<_>>>()?)};
    let mut shared=bag(v.shared_relic_bag);
    for (name,pool) in ["Event","Ancient"].into_iter().zip(&v.initial_shared_auxiliary) {
        shared[name]=json!(pool.iter().map(|r|r.name()).collect::<Vec<_>>());
    }
    let mut out=json!({"contract":"sts2-replay-checkpoint-v1","run_rng":run_rng,"players":players,
        "enemies":enemies,"round":v.combat.map(|c|c.round),"room":v.room_loaded.then(||format!("{:?}",v.run.room_type)),
        "model":v.room_model,"coord":v.run.coord.map(|(x,y)|[x,y]),"next":next,
        "rewards":reward_history,"shared_relic_bag":shared,
        "event_rng":v.event.as_ref().map(|e|rng(&e.rng)),"event_options":event_options,"shop":shop});
    normalize(&mut out);
    Ok(out)
}
pub fn project_state(v:&ReplayCheckpointView<'_>,history:&[Value],local_seat:usize)->Result<Value> {
    let mut players=Vec::new();
    for (seat,p) in v.seats.iter().enumerate() {
        players.push(json!({"seat":seat,"hp":p.hp,"gold":p.gold,"deck_size":p.deck.len(),
            "deck":cards(p.deck)?,"relics":p.relics.iter().map(|r|r.id.name()).collect::<Vec<_>>(),
            "potions":(0..p.potion_slots).map(|i|p.potions.get(i).copied().flatten()
                .map(potion).transpose().map(|p|p.map(|p|p["id"].clone()))).collect::<Result<Vec<_>>>()?}));
    }
    Ok(json!({"act":v.run.act_index+1,"floor":v.run.total_floor,"players":players,
        "replay":project(v,history,local_seat)?}))
}
/// Normalize only source-schema representation differences, never tolerances.
pub fn normalize(v:&mut Value) {
    match v {
        Value::Array(a)=>a.iter_mut().for_each(normalize),
        Value::Object(o)=>{
            o.retain(|k,v|!v.is_null() || !matches!(k.as_str(),
                "rng"|"round"|"energy"|"stars"|"room"|"model"|"coord"|"event_rng"|"event_options"|"shop"|"gold"|"relic"));
            for (k,v) in o {
                if matches!(k.as_str(),"card_rarity_odds"|"potion_reward_odds") {
                    if let Some(n)=v.as_f64() {*v=json!(f64::from(n as f32));}
                }else{normalize(v);}
            }
        }
        _=>{}
    }
}
pub fn compare(actual:&Value,expected:&Value)->Result<()> {
    let mut expected=expected.clone();normalize(&mut expected);
    fn walk(a:&Value,b:&Value,path:String)->Result<()> {
        match (a,b) {
            (Value::Object(a),Value::Object(b))=>{
                if a.keys().ne(b.keys()) {return Err(Mismatch{code:"CHECKPOINT_FIELD_SET_MISMATCH",path});}
                for (k,v) in a {walk(v,&b[k],format!("{path}/{}",k.replace('~',"~0").replace('/',"~1")))?;}
            }
            (Value::Array(a),Value::Array(b))=>{
                if a.len()!=b.len(){return Err(Mismatch{code:"CHECKPOINT_LENGTH_MISMATCH",path});}
                for (i,(a,b)) in a.iter().zip(b).enumerate(){walk(a,b,format!("{path}/{i}"))?;}
            }
            _=>if a!=b {return Err(Mismatch{code:"CHECKPOINT_DIVERGENCE",path});},
        }
        Ok(())
    }
    walk(actual,&expected,String::new())
}
