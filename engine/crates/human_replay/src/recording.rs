//! Bounded, duplicate-key rejecting recording reader. Reading is not replay
//! certification: v1 remains inspectable but must never enter training.
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sts2_sim::run::mp::Character;
use crate::Reason;
use std::io::Read;

#[derive(Clone)]
pub struct Recording {
    pub payload: Value,
    pub numeric_seed: Option<u64>,
    pub party: Vec<Character>,
}
impl Recording {
    pub fn is_v2(&self) -> bool { self.payload["schema"] == "sts2-human-run-v2" }
}

struct Bounded<'a> { depth: usize, tokens: &'a mut usize }
impl<'de> DeserializeSeed<'de> for Bounded<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value,D::Error> {
        if self.depth > 48 || *self.tokens == 0 { return Err(de::Error::custom("JSON budget")); }
        *self.tokens -= 1;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Bounded<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("bounded JSON") }
    fn visit_bool<E: de::Error>(self, x: bool) -> Result<Value,E> { Ok(x.into()) }
    fn visit_i64<E: de::Error>(self, x: i64) -> Result<Value,E> { Ok(x.into()) }
    fn visit_u64<E: de::Error>(self, x: u64) -> Result<Value,E> { Ok(x.into()) }
    fn visit_f64<E: de::Error>(self, x: f64) -> Result<Value,E> {
        serde_json::Number::from_f64(x).map(Value::Number).ok_or_else(|| E::custom("nonfinite"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value,E> { Ok(Value::Null) }
    fn visit_str<E: de::Error>(self, x: &str) -> Result<Value,E> {
        if x.len() > 8192 { return Err(E::custom("string budget")); } Ok(x.into())
    }
    fn visit_string<E: de::Error>(self, x: String) -> Result<Value,E> { self.visit_str(&x) }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value,A::Error> {
        let mut out = Vec::new();
        while let Some(x) = a.next_element_seed(Bounded { depth:self.depth+1,tokens:self.tokens })? {
            if out.len() >= 100_000 { return Err(de::Error::custom("array budget")); }
            out.push(x);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value,A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if key.len()>256 || out.len()>=1024 || out.contains_key(&key) {
                return Err(de::Error::custom("duplicate key/object budget"));
            }
            let x = a.next_value_seed(Bounded { depth:self.depth+1,tokens:self.tokens })?;
            out.insert(key,x);
        }
        Ok(Value::Object(out))
    }
}

pub fn load(bytes: &[u8]) -> Result<Recording,Reason> {
    if bytes.starts_with(&[0x1f,0x8b]) {
        if bytes.len()>16*1024*1024+65536 { return Err(Reason::InvalidRecording); }
        let mut gzip=flate2::bufread::GzDecoder::new(bytes);
        let mut raw=Vec::new();
        gzip.by_ref().take(16*1024*1024+1).read_to_end(&mut raw)
            .map_err(|_|Reason::InvalidRecording)?;
        if raw.len()>16*1024*1024 || !gzip.into_inner().is_empty() {
            return Err(Reason::InvalidRecording);
        }
        return load_json(&raw);
    }
    load_json(bytes)
}

/// Missing nullable properties are the writer's omitted null representation.
/// A present non-null value must be an unsigned integer identifying an earlier
/// input; malformed values must never become "no parent" during traversal.
pub(crate) fn validate_parents(payload:&Value)->Result<(),Reason>{
    let actions=payload["actions"].as_array().ok_or(Reason::InvalidRecording)?;
    for (seq,row) in actions.iter().enumerate(){
        let parent=&row["parent_seq"];
        if !parent.is_null()&&parent.as_u64().is_none_or(|p|p>=seq as u64){
            return Err(Reason::InvalidRecording);
        }
    }
    Ok(())
}

fn load_json(bytes: &[u8]) -> Result<Recording,Reason> {
    load_json_mode(bytes, false)
}

/// Run-start envelope, before any original input or terminal certificate exists.
/// This is deliberately separate from completed-recording admission.
pub fn load_run_start(bytes: &[u8]) -> Result<Recording,Reason> {
    load_json_mode(bytes, true)
}

fn load_json_mode(bytes: &[u8], start: bool) -> Result<Recording,Reason> {
    let invalid = || Reason::InvalidRecording;
    if bytes.len()>16*1024*1024 { return Err(invalid()); }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let mut tokens = 2_000_000;
    let payload = Bounded {depth:0,tokens:&mut tokens}.deserialize(&mut d).map_err(|_|invalid())?;
    d.end().map_err(|_|invalid())?;
    if !matches!(payload["schema"].as_str(),Some("sts2-human-run-v1"|"sts2-human-run-v2")) ||
        payload["action_schema"]!="sts2-native-input-v1" ||
        payload["game_dll_sha256"]!="0861bfa1df347538d932f22d580e75420f08082792eb914e53b4882764acdbe9" {
        return Err(invalid());
    }
    if payload["schema"]=="sts2-human-run-v1" {
        if start { return Err(invalid()); }
        return Ok(Recording {payload,numeric_seed:None,party:Vec::new()});
    }
    let numeric_seed = payload["input_seed"].as_str().ok_or_else(invalid)?
        .parse::<u64>().map_err(|_|invalid())?;
    if payload["seed"]!=payload["input_seed"] { return Err(invalid()); }
    let roster = payload["party"].as_array().ok_or_else(invalid)?;
    if !(1..=4).contains(&roster.len()) || payload["party_size"].as_u64()!=Some(roster.len() as u64) {
        return Err(invalid());
    }
    let mut party=Vec::new();
    for (seat,p) in roster.iter().enumerate() {
        if p["seat"].as_u64()!=Some(seat as u64) { return Err(invalid()); }
        party.push(match p["character"].as_str() {
            Some("IRONCLAD")=>Character::Ironclad,Some("SILENT")=>Character::Silent,
            Some("DEFECT")=>Character::Defect,Some("REGENT")=>Character::Regent,
            Some("NECROBINDER")=>Character::Necrobinder,_=>return Err(invalid()),
        });
    }
    if start {
        if !payload["initial_state"]["replay"].is_object() || !payload["setup"].is_object()
            || payload["ascension"].as_u64().is_none_or(|a|a>10)
            || payload["local_seat"].as_u64().is_none_or(|s|s>=party.len() as u64)
            || (!payload["actions"].is_null() && payload["actions"].as_array().is_none_or(|a|!a.is_empty())) {
            return Err(invalid());
        }
        let mut payload=payload;
        payload["actions"]=Value::Array(Vec::new());
        return Ok(Recording {payload,numeric_seed:Some(numeric_seed),party});
    }
    let actions=payload["actions"].as_array().ok_or_else(invalid)?;
    if actions.is_empty() || actions.len()>100_000 ||
        !payload["initial_state"]["replay"].is_object() ||
        !payload["result"]["terminal_state"]["replay"].is_object() ||
        !payload["setup"].is_object() { return Err(invalid()); }
    validate_parents(&payload)?;
    for (seq,row) in actions.iter().enumerate() {
        if row["seq"].as_u64()!=Some(seq as u64) ||
            row["action"]["player"].as_u64().is_none_or(|s|s>=party.len() as u64) ||
            row["action"]["kind"].as_str().is_none() ||
            (!row["after"]["replay"].is_object() && !row["before"].is_object()) {
            return Err(invalid());
        }
        let action=&row["action"];
        if action["kind"]=="PickCard" && action["reward_type"]=="card" {
            let offered=action["offered_cards"].as_array().ok_or_else(invalid)?;
            let index=action["index"].as_u64().ok_or_else(invalid)?;
            if offered.get(usize::try_from(index).map_err(|_|invalid())?)!=Some(&action["card"]) {
                return Err(invalid());
            }
        }
    }
    let floors=payload["floor_digests"].as_array().ok_or_else(invalid)?;
    // Training admission (A10 only) is enforced by verify, not by this reader.
    if floors.is_empty() || floors.len()>1000 || payload["ascension"].as_u64().is_none_or(|a|a>10) ||
        payload["local_seat"].as_u64().is_none_or(|s|s>=party.len() as u64) {
        return Err(invalid());
    }
    Ok(Recording {payload,numeric_seed:Some(numeric_seed),party})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_and_duplicate_json_are_rejected_before_replay() {
        for bytes in [b"{}".as_slice(),b"{\"schema\":\"sts2-human-run-v2\"}",
            b"{\"x\":0,\"x\":1}",b"{}{}",b"\xff"] {
            assert!(matches!(load(bytes),Err(Reason::InvalidRecording)));
        }
        assert!(load(format!("{}0{}","[".repeat(50),"]".repeat(50)).as_bytes()).is_err());
    }
    #[test]
    fn gzip_members_tails_and_bombs_are_rejected() {
        use std::io::Write;
        let compress=|raw:&[u8]| {
            let mut e=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::fast());
            e.write_all(raw).unwrap();e.finish().unwrap()
        };
        let mut tail=compress(b"{}");tail.push(0);
        assert!(load(&tail).is_err());
        let mut members=compress(b"{}");members.extend(compress(b"{}"));
        assert!(load(&members).is_err());
        assert!(load(&compress(&vec![b' ';16*1024*1024+1])).is_err());
    }
}
