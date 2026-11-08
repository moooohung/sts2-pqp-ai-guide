//! Shared late CardFactory reward-option hooks, in owner inventory order.
use crate::{card::{Card,CId},relic::Relic,tables::*};
use super::manager::RunManager;

pub(super) fn apply(
    relics:&mut [Relic],cards:&mut [Card],owner:CId,room:Option<RoomType>,
    is_reward:bool,next_id:&mut u32,niche:&mut crate::rng::Rng,
    mut upgrade:impl FnMut(&mut Card,&mut u32)->Result<(),&'static str>,
)->Result<(),&'static str> {
    for relic in relics.iter_mut().filter(|r|r.owner==owner&&r.enabled) {
        let id=relic.id;
        if id==RelicId::WingCharm {
            let candidates:Vec<_>=cards.iter().enumerate().filter_map(|(i,c)|
                RunManager::event_can_enchant(c,"Swift").then_some(i)).collect();
            if !candidates.is_empty() {
                let index=candidates[niche.choice_index(candidates.len())];
                *next_id+=1;cards[index].instance_id=*next_id;
                if !crate::card::enchant::apply_static_enchantment(&mut cards[index],"Swift",1) {
                    return Err("unsupported Swift reward enchantment");
                }
            }
            continue;
        }
        if matches!(id,RelicId::LavaLamp|RelicId::SilverCrucible) {
            let upgrade_all=if id==RelicId::LavaLamp {
                let room=room.ok_or("Lava Lamp reward requires room context")?;
                matches!(room,RoomType::Monster|RoomType::Elite|RoomType::Boss)&&!relic.flag1
            } else {is_reward&&relic.counter<3};
            if upgrade_all {
                for card in cards.iter_mut().filter(|c|!c.upgraded) {upgrade(card,next_id)?;}
                if id==RelicId::SilverCrucible {
                    relic.counter+=1;
                    if relic.counter>=3&&relic.counter2>0 {relic.enabled=false;}
                }
            }
            continue;
        }
        let glam=id==RelicId::Glitter||(id==RelicId::SilkenTress&&is_reward);
        for card in cards.iter_mut() {
            let egg=matches!((id,card.card_type),
                (RelicId::FrozenEgg,CardType::Power)|(RelicId::MoltenEgg,CardType::Attack)|(RelicId::ToxicEgg,CardType::Skill));
            if egg&&!card.upgraded {upgrade(card,next_id)?;}
            let enchant=if glam {Some(("Glam",1))}
                else if id==RelicId::FresnelLens {Some(("Nimble",2))}else{None};
            if let Some((name,amount))=enchant {
                if RunManager::event_can_enchant(card,name) {
                    *next_id+=1;card.instance_id=*next_id;
                    if !crate::card::enchant::apply_static_enchantment(card,name,amount) {
                        return Err("unsupported reward enchantment");
                    }
                }
            }
        }
        if id==RelicId::SilkenTress&&is_reward {relic.enabled=false;}
    }
    Ok(())
}
