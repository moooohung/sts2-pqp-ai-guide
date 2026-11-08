use super::MpRoster;
use crate::{card::Card, combat::RunCtx, tables::RelicId};

/// CardPileCmd.Add's inventory-modification phase is owner-local, while
/// template identities belong to the shared run. Never install a solo manager.
pub(super) fn add(roster: &mut MpRoster, shared: &mut RunCtx, owner: usize, mut card: Card)
    -> Result<(), &'static str> {
    let player = roster.player(owner).ok_or("invalid card owner")?;
    if card.owner as usize != owner { return Err("reward card owner mismatch"); }
    let relics: Vec<_> = player.relics.iter().filter(|r| r.enabled).map(|r| r.id).collect();
    let owner_inventory = player.relics.clone();
    shared.next_card_id = shared.next_card_id.max(roster.next_card_id);
    // The shared solo helper verifies MoltenEgg's actual owner. Present this
    // recipient's inventory for the synchronous modification call, then restore
    // the shared projection on both success and refusal. Allocation/ref-cache
    // state remains in the one shared run, never in a cloned solo manager.
    let previous = std::mem::replace(&mut shared.relics, owner_inventory);
    let result = relics.into_iter().try_for_each(|relic|
        super::super::manager::RunManager::modify_inventory_card(shared, &mut card, relic));
    shared.relics = previous;
    result?;
    roster.next_card_id = roster.next_card_id.max(shared.next_card_id);
    card.floor_added_to_deck = Some(shared.total_floor); // CardPileCmd.Add
    roster.player_mut(owner).unwrap().deck.push(card);
    after_deck_add(roster, shared, owner, true)?;
    Ok(())
}

fn after_deck_add(roster: &mut MpRoster, shared: &mut RunCtx, owner: usize, allow_bing_bong: bool)
    -> Result<(), &'static str> {
    // C# AfterCardChangedPiles listeners run in inventory order. LuckyFysh.cs,
    // DarkstonePeriapt.cs and BookOfFiveRings.cs are synchronous owner-local
    // state changes; BingBong.cs clones exactly once because its clone is added
    // with `clonedBy = this` and therefore does not recursively clone again.
    let added_index = roster.player(owner).ok_or("invalid card owner")?.deck.len()
        .checked_sub(1).ok_or("added deck card missing")?;
    let (is_curse, owner_alive) = {
        let p = roster.player(owner).unwrap();
        (p.deck[added_index].card_type == crate::tables::CardType::Curse, p.hp > 0)
    };
    let relics: Vec<_> = roster.player(owner).unwrap().relics.iter()
        .enumerate().filter(|(_, r)| r.enabled).map(|(i, r)| (i, r.id)).collect();
    let mut clone = false;
    for (relic_index, relic_id) in relics {
        match relic_id {
            RelicId::LuckyFysh => {
                roster.player_mut(owner).unwrap().gain_gold(shared,15);
            }
            RelicId::DarkstonePeriapt if is_curse => {
                // DarkstonePeriapt.AfterCardChangedPiles -> GainMaxHp(6).
                let p = roster.player_mut(owner).unwrap();
                p.max_hp += 6;
                p.hp = (p.hp + 6).min(p.max_hp);
            }
            RelicId::BookOfFiveRings if owner_alive => {
                // BookOfFiveRings.cs:77-91: every fifth owner Deck entry heals 20.
                let heal = {
                    let relic = &mut roster.player_mut(owner).unwrap().relics[relic_index];
                    relic.counter += 1;
                    relic.counter % 5 == 0
                };
                if heal {
                    let p = roster.player_mut(owner).unwrap();
                    p.hp = (p.hp + 20).min(p.max_hp);
                }
            }
            RelicId::BingBong if allow_bing_bong => clone = true,
            _ => {}
        }
    }
    if clone {
        let mut cloned = roster.player(owner).unwrap().deck[added_index].clone();
        shared.next_card_id = shared.next_card_id.max(roster.next_card_id) + 1;
        roster.next_card_id = shared.next_card_id;
        cloned.instance_id = roster.next_card_id;
        cloned.floor_added_to_deck = Some(shared.total_floor);
        roster.player_mut(owner).unwrap().deck.push(cloned);
        after_deck_add(roster, shared, owner, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{relic::Relic, run::mp::{Character, MpRun, PlayerSpec}, tables::CardId};

    fn party() -> MpRun {
        MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap()
    }

    #[test]
    fn owner_after_deck_add_relics_follow_csharp_and_bing_bong_clones_once() {
        let mut run = party();
        for id in [RelicId::LuckyFysh, RelicId::DarkstonePeriapt,
            RelicId::BookOfFiveRings, RelicId::BingBong] {
            let mut relic = Relic::new(id);
            relic.owner = 1;
            run.roster.player_mut(1).unwrap().relics.push(relic);
        }
        run.roster.player_mut(1).unwrap().relics.iter_mut()
            .find(|r| r.id == RelicId::BookOfFiveRings).unwrap().counter = 3;
        let before_gold = run.roster.player(1).unwrap().gold;
        let before_max = run.roster.player(1).unwrap().max_hp;
        run.roster.player_mut(1).unwrap().hp = 10;
        run.roster.next_card_id += 1;
        let mut card = Card::from_factory(CardId::Injury, false, run.roster.next_card_id).unwrap();
        card.owner = 1;
        let before_len = run.roster.player(1).unwrap().deck.len();
        add(&mut run.roster, &mut run.world.run, 1, card).unwrap();
        let p = run.roster.player(1).unwrap();
        assert_eq!(p.deck.len(), before_len + 2);
        assert_eq!(p.gold, before_gold + 30);
        assert_eq!(p.max_hp, before_max + 12);
        assert_eq!(p.hp, 42);
        assert_eq!(p.relics.iter().find(|r| r.id == RelicId::BookOfFiveRings).unwrap().counter, 5);
        assert_ne!(p.deck[p.deck.len() - 2].instance_id, p.deck[p.deck.len() - 1].instance_id);
    }
}
