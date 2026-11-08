use super::MpRoster;
use crate::{combat::RunCtx, relic::Relic, tables::{
    CardRarity, CardTag, CardType, RelicId, RelicRarity, RELIC_INFO,
}};

impl MpRoster {
    /// Immediate owner-only inventory commands. The normal Common/Uncommon/Rare
    /// grab bags have exactly seven C# `AfterObtained` models: PotionBelt,
    /// Strawberry, WarPaint, Whetstone, Pear, Mango and OldCoin. Everything else
    /// in those three bags has no pickup callback, so it is safe to attach the
    /// relic directly. Ancient/Event/Shop/Boss models still require their own
    /// run-layer continuation.
    pub fn obtain_inventory_relic(&mut self, shared: &mut RunCtx, owner: usize, id: RelicId)
        -> Result<(), &'static str> {
        let normal_bag = matches!(RELIC_INFO[id.index()].rarity,
            RelicRarity::Common | RelicRarity::Uncommon | RelicRarity::Rare);
        if !normal_bag && id != RelicId::NeowsTalisman && !RELIC_INFO[id.index()].stackable {
            return Err("multiplayer pickup requires a run continuation");
        }
        let next_card_id = self.next_card_id;
        let p = self.player_mut(owner).ok_or("invalid pickup owner")?;
        shared.next_card_id = shared.next_card_id.max(next_card_id);
        // Existing permanent-upgrade implementation preserves enchantments and
        // accumulated card values. Swap ONLY this owner's deck into its context;
        // never create a solo manager with another map/RNG schedule.
        if id == RelicId::PotionBelt {
            // PotionBelt.cs:19-22.
            p.max_potion_slots += 2;
            p.potions.resize(p.max_potion_slots, None);
        } else if matches!(id, RelicId::Whetstone | RelicId::WarPaint | RelicId::NeowsTalisman) {
            std::mem::swap(&mut shared.deck, &mut p.deck);
            if matches!(id, RelicId::Whetstone | RelicId::WarPaint) {
                // Whetstone.cs / WarPaint.cs `AfterObtained` and
                // ListExtensions.StableShuffle: sort first, shuffle on the
                // shared run Niche stream, then take at most two eligible cards.
                let wanted = if id == RelicId::Whetstone { CardType::Attack } else { CardType::Skill };
                let mut indices: Vec<_> = (0..shared.deck.len()).filter(|&i|
                    shared.deck[i].card_type == wanted && shared.is_deck_card_upgradable(i))
                    .map(|i| i as crate::card::KRef).collect();
                crate::flow::dotnet_sort_cards(&mut indices, &shared.deck);
                shared.rngs.niche.shuffle(&mut indices);
                for i in indices.into_iter().take(2) { shared.upgrade_deck_card(i as usize); }
            } else {
                // NeowsTalisman.cs:18-28: last Basic Strike and last Basic Defend.
                let strike = shared.deck.iter().rposition(|c|
                    c.rarity == CardRarity::Basic && c.has_tag(CardTag::Strike));
                let defend = shared.deck.iter().rposition(|c|
                    c.rarity == CardRarity::Basic && c.has_tag(CardTag::Defend));
                for i in [strike, defend].into_iter().flatten() { shared.upgrade_deck_card(i); }
            }
            std::mem::swap(&mut shared.deck, &mut p.deck);
        } else {
            // Strawberry.cs / Pear.cs / Mango.cs use CreatureCmd.GainMaxHp,
            // which heals by the same amount as the actual max-HP increase.
            let hp = match id {
                RelicId::Strawberry => 7,
                RelicId::Pear => 10,
                RelicId::Mango => 14,
                _ => 0,
            };
            if hp != 0 {
                p.max_hp += hp;
                p.hp = (p.hp + hp).min(p.max_hp);
            }
            // OldCoin.cs: AfterObtained -> PlayerCmd.GainGold(300).
            if id == RelicId::OldCoin { p.gain_gold(shared,300); }
        }
        let mut relic = Relic::new(id);
        relic.owner = owner as u8;
        relic.floor_added_to_deck = shared.total_floor;
        p.relics.push(relic);
        self.next_card_id = self.next_card_id.max(shared.next_card_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, MpRun, PlayerSpec};

    fn party() -> MpRun {
        MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap()
    }

    #[test]
    fn normal_bag_max_hp_gold_and_slots_match_csharp_pickup_commands() {
        for (id, delta) in [(RelicId::Strawberry, 7), (RelicId::Pear, 10), (RelicId::Mango, 14)] {
            let mut run = party();
            let owner = run.roster.player_mut(1).unwrap();
            owner.hp = 30;
            let max_hp = owner.max_hp;
            run.roster.obtain_inventory_relic(&mut run.world.run, 1, id).unwrap();
            let owner = run.roster.player(1).unwrap();
            assert_eq!(owner.max_hp, max_hp + delta);
            assert_eq!(owner.hp, 30 + delta);
        }

        let mut run = party();
        let gold = run.roster.player(1).unwrap().gold;
        run.roster.obtain_inventory_relic(&mut run.world.run, 1, RelicId::OldCoin).unwrap();
        assert_eq!(run.roster.player(1).unwrap().gold, gold + 300);
        run.roster.obtain_inventory_relic(&mut run.world.run, 1, RelicId::PotionBelt).unwrap();
        assert_eq!(run.roster.player(1).unwrap().max_potion_slots, 5);
        assert_eq!(run.roster.player(1).unwrap().potions.len(), 5);
    }

    #[test]
    fn normal_bag_upgrade_relics_use_owner_deck_and_shared_niche_rng() {
        let mut run = party();
        let other_before = format!("{:?}", run.roster.player(0).unwrap());
        let skill_before = run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.card_type == CardType::Skill && c.upgraded).count();
        let attack_before = run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.card_type == CardType::Attack && c.upgraded).count();
        let niche = run.world.run.rngs.niche.counter;

        run.roster.obtain_inventory_relic(&mut run.world.run, 1, RelicId::WarPaint).unwrap();
        let skill_after = run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.card_type == CardType::Skill && c.upgraded).count();
        assert_eq!(skill_after - skill_before, 2);
        assert!(run.world.run.rngs.niche.counter > niche);

        run.roster.obtain_inventory_relic(&mut run.world.run, 1, RelicId::Whetstone).unwrap();
        let attack_after = run.roster.player(1).unwrap().deck.iter()
            .filter(|c| c.card_type == CardType::Attack && c.upgraded).count();
        assert_eq!(attack_after - attack_before, 2);
        assert_eq!(format!("{:?}", run.roster.player(0).unwrap()), other_before);
    }

    #[test]
    fn normal_bag_relic_without_pickup_callback_is_attached_to_owner() {
        let mut run = party();
        run.roster.obtain_inventory_relic(&mut run.world.run, 1, RelicId::VenerableTeaSet).unwrap();
        assert!(run.roster.player(1).unwrap().relics.iter().any(|r| r.id == RelicId::VenerableTeaSet));
        assert!(!run.roster.player(0).unwrap().relics.iter().any(|r| r.id == RelicId::VenerableTeaSet));
    }
}
