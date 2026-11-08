//! Presentation-only state; never export generic private relic counters.
use serde_json::{Value, json};
use sts2_sim::{combat::Combat, relic::Relic, tables::RelicId};
use sts2_sim::{run::NativeEventOffer, tables::{CardId, CardType, PotionRarity, POTION_INFO}};

pub(super) fn settlement(source:&super::RealSource<'_>,actor:u8)->Result<Value,String> {
    let phase=source.phase(actor);
    let combat=source.game.combat().filter(|_|phase=="COMBAT");
    let players=source.inventory["seats"].as_array().ok_or("missing public settlement seats")?
        .iter().enumerate().map(|(owner,seat)| {
            let mut player=json!({"actor":owner,"character":match &source.game {
                super::Game::Solo(g)=>g.native_character().name(),
                super::Game::Party(g)=>g.roster.players()[owner].character.name(),
            }});
            for key in ["hp","max_hp","mhp","gold","relics","relic_state","pot","ps","pv"] {
                if let Some(value)=seat.get(key) {player[key]=value.clone();}
            }
            if let Some(c)=combat {super::combat_player_public(c,owner as u8,&mut player);}
            player
        }).collect::<Vec<_>>();
    let mut state=json!({"ph":phase,"fl":source.floor,"act":source.inventory["act"],
        "party_size":source.seats,"actor":actor,"players":players});
    if let Some(c)=combat {
        state["turn"]=json!(c.round);state["foes"]=json!(super::foes(c,actor));
        state["foe_ids"]=json!(c.enemies.as_slice());
        state["summons"]=json!((0..source.seats).flat_map(|owner|super::pets(c,owner as u8)).collect::<Vec<_>>());
    }
    Ok(state)
}

pub(super) fn event_offer(offer:Option<NativeEventOffer>, detail:(Option<CardId>,Option<CardType>), option:&mut Value) {
    match offer {
        Some(NativeEventOffer::Pool(pool))=>option["pool_id"]=json!(pool.name()),
        Some(NativeEventOffer::Potion(potion))=>{
            let rarity=POTION_INFO[potion.index()].rarity;
            option["event_option_key"]=json!("POTION");
            option["potion_id"]=json!(potion.name());
            option["potion_rarity"]=json!(rarity.name());
            option["card_rarity"]=json!(match rarity {
                PotionRarity::Common|PotionRarity::Token=>"Common",
                PotionRarity::Uncommon=>"Uncommon",_=>"Rare"});
            option["card_type"]=json!(detail.1.map(|kind|kind.name()));
        }
        _=>{}
    }
}

pub(super) fn relic(relic: &Relic, combat: Option<&Combat>) -> Value {
    let amount=crate::encoder::relic_counter(relic,combat);
    let mut state=json!({"id":relic.id.name(),"show_counter":amount.is_some(),"display_amount":amount});
    // OrnamentalFan.UpdateDisplay. Animation-only IsActivating is settled at
    // decision boundaries; its retained attack count is hidden outside combat.
    if relic.id==RelicId::OrnamentalFan {
        state["status"]=json!(if combat.is_some() && !relic.flag1 && relic.counter%3==2 {"active"} else {"normal"});
    }
    if matches!(relic.id,RelicId::BoneTea|RelicId::EmberTea|RelicId::TeaOfDiscourtesy) {
        let remaining=(if relic.id==RelicId::EmberTea {5} else {1}-relic.counter).max(0);
        state["vars"]=json!({"Combats":remaining});
        state["status"]=json!(if remaining==0 {"disabled"} else {"normal"});
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn potion_trade_exports_the_disclosed_type_and_rarity_without_rolling_cards() {
        let mut option=json!({});
        event_offer(Some(NativeEventOffer::Potion(sts2_sim::tables::PotionId::Firepotion)),
            (None,Some(CardType::Skill)),&mut option);
        assert_eq!(option["potion_id"],"FirePotion");
        assert_eq!(option["event_option_key"],"POTION");
        assert_eq!(option["card_rarity"],"Common");
        assert_eq!(option["card_type"],CardType::Skill.name());
    }
}
