//! Trainer/browser party sampling, independent of the caller's RNG storage.
use std::collections::BTreeMap;
pub fn weighted(weights:&[f64],uniform:f64)->usize{
    let mut point=uniform*weights.iter().sum::<f64>();
    for (i,w) in weights.iter().enumerate(){point-=w;if point<0.{return i}}
    weights.iter().rposition(|w|*w>0.).unwrap()
}
pub fn party(characters:&BTreeMap<String,f64>,parties:&[f64],mut uniform:impl FnMut()->f64)->Vec<String>{
    let names=characters.keys().collect::<Vec<_>>();
    let weights=characters.values().copied().collect::<Vec<_>>();
    let seats=weighted(parties,uniform())+1;
    (0..seats).map(|_|String::clone(names[weighted(&weights,uniform())])).collect()
}
#[cfg(test)]
mod tests{
    #[test]
    fn party_uses_sorted_weighted_draws_with_replacement(){
        let pool=BTreeMap::from([("Silent".to_owned(),1.),("Ironclad".to_owned(),3.)]);
        let mut draws=[0.9,0.,0.999].into_iter();
        assert_eq!(super::party(&pool,&[0.,1.,0.,0.],||draws.next().unwrap()),["Ironclad","Silent"]);
        assert_eq!(super::party(&pool,&[0.,1.,0.,0.],||0.),["Ironclad","Ironclad"]);
    }
    use super::BTreeMap;
}
