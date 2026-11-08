//! The checkpoint's trainer distribution, sampled in the trainer's BTreeMap order.
use super::*;
use std::collections::BTreeMap;
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct TrainingMix {
 pub character_weights:BTreeMap<String,f64>,pub party_weights:Vec<f64>,
 #[serde(default="default_ascension")]
 pub ascension_min:u8,
 #[serde(default="a10_weights")]
 pub ascension_weights:Vec<f64>,
}
fn a10_weights()->Vec<f64>{vec![1.]}
impl TrainingMix{
 pub fn validate(&self)->Result<(),String>{
  if self.ascension_min!=10 || self.ascension_weights.len()!=1
   || !self.ascension_weights[0].is_finite() || self.ascension_weights[0]<=0. {
   return Err("AI-play training_mix requires ascension 10 only".into())
  }
  let registry=Namespace::Character.registry().ok_or("character registry")?;
  if self.character_weights.len()!=registry.count || registry.names.iter().any(|name|!self.character_weights.contains_key(*name))
   || self.character_weights.values().any(|w|!w.is_finite()||*w<=0.)
   || self.party_weights.len()!=MAX_SEATS || self.party_weights.iter().any(|w|!w.is_finite()||*w<0.)
   || !self.party_weights.iter().sum::<f64>().is_finite() || self.party_weights.iter().sum::<f64>()<=0.
   || !self.character_weights.values().sum::<f64>().is_finite(){return Err("invalid trainer sampling distribution".into())}
  Ok(())
 }
 pub fn party(&self,seed:u64)->Vec<String>{
  let mut rng=Rng(seed);
  sts2_native_obs::sampling::party(&self.character_weights,&self.party_weights,||rng.uniform())
   .into_iter().map(|name|name.to_ascii_uppercase()).collect()
 }
}
pub fn characters()->Vec<Value>{
 Namespace::Character.registry().unwrap().names.iter().map(|name|{
  let id=name.to_ascii_uppercase();
  let ko=match id.as_str(){"IRONCLAD"=>"아이언클래드","SILENT"=>"사일런트","DEFECT"=>"디펙트",
   "REGENT"=>"리젠트","NECROBINDER"=>"네크로바인더",_=>*name};
  json!({"id":id,"nameKo":ko})
 }).collect()
}
