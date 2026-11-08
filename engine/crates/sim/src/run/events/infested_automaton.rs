//! CardFactory.CreateForReward with ForNonCombatWithDefaultOdds: base rarity
//! without pity mutation, promote unavailable rarities, no upgrade roll.
use crate::{rng::Rng, tables::{card_info, CardId, CardRarity, CardType}};

pub(crate) fn draw(character: crate::character::Character, multiplayer: bool, ascension: i32, rng: &mut Rng,
    kind: Option<CardType>, cost: Option<i32>, costs_x: Option<bool>)
    -> Result<CardId, &'static str> {
    let roll = rng.next_float(1.0) as f32;
    let rare = if ascension >= 7 { 0.0149f32 } else { 0.03f32 };
    let mut rarity = if roll < rare { CardRarity::Rare }
        else if roll < rare + 0.37f32 { CardRarity::Uncommon }
        else { CardRarity::Common };
    let initial = rarity;
    let pool: Vec<_> = super::super::card_pools::cards(character.into(), multiplayer)?.into_iter()
        .filter(|id| card_info(*id, false).is_some_and(|c|
            kind.is_none_or(|v| c.card_type == v) && cost.is_none_or(|v| c.cost == v)
                && costs_x.is_none_or(|v| c.x_cost == v))).collect();
    loop {
        let candidates: Vec<_> = pool.iter().copied()
            .filter(|id| card_info(*id, false).is_some_and(|c| c.rarity == rarity)).collect();
        if !candidates.is_empty() { return Ok(candidates[rng.choice_index(candidates.len())]); }
        rarity = match rarity {
            CardRarity::Common => CardRarity::Uncommon,
            CardRarity::Uncommon => CardRarity::Rare,
            CardRarity::Rare => CardRarity::Common,
            _ => return Err("unsupported reward rarity"),
        };
        if rarity == initial { return Err("card pool exhausted"); }
    }
}
