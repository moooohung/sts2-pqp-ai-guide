//! C# Models/Events/Tanx.cs GenerateInitialOptions and Claws/TriBoomerang/
//! TanxsWhistle.AfterObtained. Shared offer construction for solo and party seats.
use crate::{card::Card,tables::*};
use super::{events::{EventState,EventOption},manager::RunManager};
pub(super) fn options(event:&mut EventState,deck:&[Card]){
    let mut pool=vec![RelicId::Claws,RelicId::Crossbow,RelicId::IronClub,RelicId::MeatCleaver,
        RelicId::Sai,RelicId::SpikedGauntlets,RelicId::TanxsWhistle,RelicId::ThrowingAxe,RelicId::WarHammer];
    if deck.iter().filter(|c|RunManager::event_can_enchant(c,"Instinct")).count()>=3{
        pool.push(RelicId::TriBoomerang);
    }
    event.rng.shuffle(&mut pool);event.options.clear();event.relics.clear();
    for (id,option) in pool.into_iter().take(3).zip(["relic_1","relic_2","relic_3"]){
        event.relics.push(id);
        event.options.push(EventOption::new(option,id.name(),"Gain an ancient relic"));
    }
}
pub(super) fn maul(original:&Card,id:u32)->Result<Card,&'static str>{
    let mut card=Card::from_factory(CardId::Maul,original.upgraded,id).ok_or("Maul factory missing")?;
    card.owner=original.owner;
    for &(name,amount) in &original.enchantments {
        if RunManager::event_can_enchant(&card,name.name())
            &&!crate::card::enchant::apply_static_enchantment(&mut card,name,amount){
            return Err("Claws enchantment capacity");
        }
    }
    Ok(card)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn offers_use_full_ordered_pool_before_shuffle_and_conditional_boomerang(){
        let mut event=EventState::new(crate::identity::EventId::Tanx,crate::rng::Rng::new(91));
        let deck:Vec<_>=(0..3).map(|i|Card::from_factory(CardId::StrikeIronclad,false,i).unwrap()).collect();
        let mut rng=event.rng.clone();
        let mut expected=vec![RelicId::Claws,RelicId::Crossbow,RelicId::IronClub,RelicId::MeatCleaver,
            RelicId::Sai,RelicId::SpikedGauntlets,RelicId::TanxsWhistle,RelicId::ThrowingAxe,RelicId::WarHammer,RelicId::TriBoomerang];
        rng.shuffle(&mut expected);options(&mut event,&deck);
        assert_eq!(event.relics.as_slice(),&expected[..3]);
        assert_eq!(format!("{:?}",event.rng),format!("{rng:?}"));
    }
    #[test]fn claws_preserves_upgrade_and_only_compatible_enchantments(){
        for upgraded in [false,true]{
            let mut original=Card::from_factory(CardId::StrikeIronclad,upgraded,1).unwrap();original.owner=2;
            original.apply_static_enchantment("Momentum",5);
            let card=maul(&original,2).unwrap();
            assert_eq!((card.id,card.upgraded,card.owner,card.instance_id),(CardId::Maul,upgraded,2,2));
            assert_eq!(card.enchantments.as_slice(),&[(crate::identity::EnchantmentId::Momentum,5)]);
        }
    }
}
