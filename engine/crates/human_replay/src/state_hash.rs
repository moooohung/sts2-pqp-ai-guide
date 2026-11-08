//! RecordingFormat.Hash's original C# record-property order. Dictionaries are
//! ordinal-sorted; absent/null record properties are omitted, array holes kept.
use serde_json::Value;
use sha2::{Digest,Sha256};
use crate::{checkpoint::{Mismatch,Result},recording::Recording};

#[derive(Clone,Copy)]
enum Shape { Atom,Record(&'static [(&'static str,Shape)]),Array(&'static Shape),Map(&'static Shape) }
use Shape::*;
const A:Shape=Atom;
const ATOMS:Shape=Array(&A);
const CARD:Shape=Record(&[("id",A),("upgrade",A)]);
const CARDS:Shape=Array(&CARD);
const RNG:Shape=Record(&[("counter",A),("state",ATOMS)]);
const RNGS:Shape=Map(&RNG);
const POWER:Shape=Record(&[("id",A),("amount",A)]);
const CREATURE:Shape=Record(&[("id",A),("hp",A),("max_hp",A),("block",A),
    ("powers",Array(&POWER)),("rng",RNG)]);
const RELIC:Shape=Record(&[("id",A),("status",A),("stacks",A),("display",A),("vars",Map(&A))]);
const POTION:Shape=Record(&[("id",A),("queued",A),("vars",Map(&A))]);
const BAG:Shape=Map(&ATOMS);
const SEAT:Shape=Record(&[("seat",A),("energy",A),("stars",A),("hand",CARDS),("draw",CARDS),
    ("discard",CARDS),("exhaust",CARDS),("creature",CREATURE),("rng",RNGS),
    ("relics",Array(&RELIC)),("potions",Array(&POTION)),("card_rarity_odds",A),
    ("potion_reward_odds",A),("relic_bag",BAG)]);
const OFFER:Shape=Record(&[("kind",A),("selected",A),("cards",CARDS),("model",A),("gold",A)]);
const EVENT_OPTION:Shape=Record(&[("id",A),("enabled",A),("relic",A)]);
const SHOP:Shape=Record(&[("kind",A),("model",A),("price",A),("stocked",A)]);
const REPLAY:Shape=Record(&[("contract",A),("run_rng",RNGS),("players",Array(&SEAT)),
    ("enemies",Array(&CREATURE)),("round",A),("room",A),("model",A),("coord",ATOMS),
    ("next",Array(&ATOMS)),("rewards",Array(&OFFER)),("event_rng",RNG),
    ("event_options",Array(&EVENT_OPTION)),("shop",Array(&SHOP)),("shared_relic_bag",BAG)]);
const PLAYER:Shape=Record(&[("seat",A),("hp",A),("gold",A),("deck_size",A),("deck",CARDS),
    ("relics",ATOMS),("potions",ATOMS)]);
const STATE:Shape=Record(&[("act",A),("floor",A),("players",Array(&PLAYER)),
    ("rng_counters",Map(&A)),("replay",REPLAY)]);

fn emit(value:&Value,shape:Shape,out:&mut Vec<u8>)->Result<()> {
    let bad=||Mismatch{code:"INVALID_CHECKPOINT_HASH_SHAPE",path:String::new()};
    if value.is_null() {out.extend_from_slice(b"null");return Ok(());}
    match shape {
        Atom=>{
            if value.is_array()||value.is_object(){return Err(bad());}
            let bytes=serde_json::to_vec(value).map_err(|_|bad())?;
            // Checkpoint identifiers and dynamic-variable names are invariant
            // ASCII. Other serializers/encodings need an explicit adapter.
            if !bytes.is_ascii(){return Err(Mismatch::unsupported("non-ASCII checkpoint hash"));}
            out.extend(bytes);
        }
        Record(fields)=>{
            let object=value.as_object().ok_or_else(bad)?;
            if object.keys().any(|k|!fields.iter().any(|(name,_)|name==k)){return Err(bad());}
            out.push(b'{');
            let mut comma=false;
            for &(key,kind) in fields {
                let Some(v)=object.get(key).filter(|v|!v.is_null()) else{continue};
                if comma{out.push(b',');}comma=true;
                out.extend(serde_json::to_vec(key).map_err(|_|bad())?);out.push(b':');
                emit(v,kind,out)?;
            }
            out.push(b'}');
        }
        Array(kind)=>{
            out.push(b'[');
            for (i,v) in value.as_array().ok_or_else(bad)?.iter().enumerate() {
                if i>0{out.push(b',');}emit(v,*kind,out)?;
            }
            out.push(b']');
        }
        Map(kind)=>{
            out.push(b'{');
            for (i,(key,v)) in value.as_object().ok_or_else(bad)?.iter().enumerate() {
                if !key.is_ascii(){return Err(bad());}
                if i>0{out.push(b',');}
                out.extend(serde_json::to_vec(key).map_err(|_|bad())?);out.push(b':');
                emit(v,*kind,out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}
pub fn hex(bytes:&[u8])->String {bytes.iter().map(|b|format!("{b:02x}")).collect()}
pub fn raw_sha256(bytes:&[u8])->String {hex(&Sha256::digest(bytes))}
pub fn state_hash(state:&Value)->Result<String> {
    let mut bytes=Vec::new();emit(state,STATE,&mut bytes)?;
    Ok(raw_sha256(&bytes))
}
pub fn validate(recording:&Recording)->Result<()> {
    for row in recording.payload["actions"].as_array().ok_or_else(||Mismatch::unsupported("actions"))? {
        if row["state_sha256"].as_str()!=Some(state_hash(&row["after"])?.as_str()) {
            return Err(Mismatch{code:"RECORDING_STATE_HASH_MISMATCH",path:format!("/actions/{}/after",row["seq"])});
        }
    }
    for (i,floor) in recording.payload["floor_digests"].as_array()
        .ok_or_else(||Mismatch::unsupported("floors"))?.iter().enumerate() {
        if floor["sha256"].as_str()!=Some(state_hash(&floor["state"])?.as_str()) {
            return Err(Mismatch{code:"RECORDING_STATE_HASH_MISMATCH",path:format!("/floor_digests/{i}")});
        }
    }
    Ok(())
}
