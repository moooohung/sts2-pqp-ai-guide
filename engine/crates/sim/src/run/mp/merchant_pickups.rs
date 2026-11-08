//! Awaited, actor-bound merchant relic pickups, followed by merchant restock.
use super::{MpRun,MpPhase};
use crate::{card::Card,tables::*,relic::Relic};

#[derive(Clone,Debug)]
pub enum MerchantPickupChoice {
    Deck { relic:RelicId, cards:Vec<Card>, min:usize, max:usize },
    Cards(Vec<Vec<Card>>),
    Potions(Vec<PotionId>),
}
#[derive(Clone,Debug)]
pub(super) struct MerchantPickup {
    pub choice:MerchantPickupChoice,
    pub index:usize,
    pub price:i32,
}

impl MpRun {
    pub fn merchant_pickup(&self,net:u64,seat:usize)->Result<Option<&MerchantPickupChoice>,&'static str> {
        self.roster.actor(net,seat)?;
        if self.phase!=MpPhase::Shop {return Err("not in merchant room");}
        Ok(self.merchant_pickups[seat].as_ref().map(|p|&p.choice))
    }

    pub(super) fn begin_merchant_pickup(&mut self,seat:usize,relic:RelicId,index:usize,price:i32)
        ->Result<(),&'static str> {
        let choice=match relic {
            RelicId::Orrery=>{
                let pool=super::super::card_pools::cards(self.roster.player(seat).unwrap().character.into(),true)?;
                let mut bundles=Vec::new();
                for _ in 0..5 {bundles.push(self.make_other_regular_reward_cards(seat,&pool,3,true)?);}
                MerchantPickupChoice::Cards(bundles)
            }
            RelicId::Cauldron=>MerchantPickupChoice::Potions(
                super::rewards::generate_potions(self.roster.player_mut(seat).unwrap(),5)?),
            _=>{
                let enchant=match relic {
                    RelicId::PunchDagger=>Some("Momentum"),RelicId::RoyalStamp=>Some("RoyallyApproved"),
                    RelicId::Kifuda=>Some("Adroit"),_=>None,
                };
                let mut cards:Vec<_>=self.roster.player(seat).unwrap().deck.iter().filter(|c|
                    enchant.map_or(c.card_type!=CardType::Quest,|name|
                        super::super::manager::RunManager::event_can_enchant(c,name))).cloned().collect();
                if relic==RelicId::RoyalStamp {self.world.run.rngs.niche.shuffle(&mut cards);}
                let min=usize::from(relic!=RelicId::Kifuda);
                let max=cards.len().min(if relic==RelicId::Kifuda {3}else{1});
                MerchantPickupChoice::Deck{relic,cards,min,max}
            }
        };
        self.merchant_pickups[seat]=Some(MerchantPickup{choice,index,price});
        if let Some(MerchantPickupChoice::Deck{cards,min,..})=self.merchant_pickup(
            self.roster.player(seat).unwrap().net_id,seat)? {
            if cards.len()<=*min {
                let selected:Vec<_>=(0..cards.len()).collect();
                return self.select_merchant_pickup(self.roster.player(seat).unwrap().net_id,seat,&selected);
            }
        }
        Ok(())
    }

    pub fn select_merchant_pickup(&mut self,net:u64,seat:usize,indices:&[usize])->Result<(),&'static str> {
        self.roster.actor(net,seat)?;
        if self.phase!=MpPhase::Shop {return Err("not in merchant room");}
        let mut next=self.clone();
        let mut pending=next.merchant_pickups[seat].take().ok_or("merchant pickup missing")?;
        let complete=match &mut pending.choice {
            MerchantPickupChoice::Deck{relic,cards,min,max}=>{
                let mut unique=indices.to_vec();unique.sort_unstable();unique.dedup();
                if unique.len()!=indices.len() || indices.len()<(*min).min(cards.len())
                    || indices.len()>*max || indices.iter().any(|&i|i>=cards.len()) {
                    return Err("invalid merchant pickup cards");
                }
                for &index in indices {
                    let card=&cards[index];
                    if *relic==RelicId::DollysMirror {
                        let mut copy=card.clone();next.roster.next_card_id+=1;
                        copy.instance_id=next.roster.next_card_id;
                        super::cards::add(&mut next.roster,&mut next.world.run,seat,copy)?;
                    } else {
                        let (name,amount)=match relic {
                            RelicId::PunchDagger=>("Momentum",5),RelicId::RoyalStamp=>("RoyallyApproved",1),
                            RelicId::Kifuda=>("Adroit",3),_=>return Err("invalid merchant enchantment"),
                        };
                        let original=next.roster.player_mut(seat).unwrap().deck.iter_mut()
                            .find(|c|c.instance_id==card.instance_id).ok_or("merchant pickup card missing")?;
                        if !crate::card::enchant::apply_static_enchantment(original,name,amount) {
                            return Err("merchant pickup enchantment unavailable");
                        }
                    }
                }
                true
            }
            MerchantPickupChoice::Cards(bundles)=>{
                if indices.len()>1 || indices.first().is_some_and(|&i|i>=bundles[0].len()) {
                    return Err("invalid merchant reward card");
                }
                let cards=bundles.remove(0);
                if let Some(&index)=indices.first() {
                    super::cards::add(&mut next.roster,&mut next.world.run,seat,cards[index].clone())?;
                }
                bundles.is_empty()
            }
            MerchantPickupChoice::Potions(potions)=>{
                if indices.len()>1 || indices.first().is_some_and(|&i|i!=0) {return Err("invalid merchant potion choice");}
                if !indices.is_empty() {
                    let player=next.roster.player_mut(seat).unwrap();
                    if !player.relics.iter().any(|r|r.id==RelicId::Sozu && r.enabled) {
                        let slot=player.potions.iter_mut().find(|s|s.is_none()).ok_or("potion inventory full")?;
                        *slot=Some(potions[0]);
                        next.mark_local_potion_seen(seat,potions[0]);
                    }
                }
                potions.remove(0);potions.is_empty()
            }
        };
        if complete {next.finish_merchant_relic_purchase(seat,pending.index,pending.price)?;}
        else {next.merchant_pickups[seat]=Some(pending);}
        *self=next;
        Ok(())
    }

    pub(super) fn attach_merchant_pickup(&mut self,seat:usize,relic:RelicId) {
        let mut r=Relic::new(relic);r.owner=seat as u8;r.floor_added_to_deck=self.world.run.total_floor;
        self.roster.player_mut(seat).unwrap().relics.push(r);
        self.record_relic_seen(seat,relic);
    }
}
