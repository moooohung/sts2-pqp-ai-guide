//! C# Kaleidoscope.AfterObtained, NeowsBones.AfterObtained / Neow.AllPossibleOptions.
use crate::character::CardPool;
use super::*;

pub(super) const NEOW_RELICS: &[RelicId] = &[
    RelicId::CursedPearl, RelicId::DowsingRod, RelicId::HeftyTablet,
    RelicId::LargeCapsule, RelicId::LeafyPoultice, RelicId::NeowsBones,
    RelicId::NeowsSacrifice, RelicId::PrecariousShears, RelicId::SilkenTress,
    RelicId::SilverCrucible, RelicId::ArcaneScroll, RelicId::BoomingConch,
    RelicId::FishingRod, RelicId::GoldenPearl, RelicId::Kaleidoscope,
    RelicId::LeadPaperweight, RelicId::LostCoffer, RelicId::MassiveScroll,
    RelicId::NeowsTorment, RelicId::NewLeaf, RelicId::PhialHolster,
    RelicId::PreciseScissors, RelicId::ScrollBoxes, RelicId::WingedBoots,
    RelicId::LavaRock, RelicId::NeowsTalisman, RelicId::NutritiousOyster,
    RelicId::Pomander, RelicId::SmallCapsule, RelicId::StoneHumidifier,
];

impl RunManager {
    pub(super) fn offer_kaleidoscope(&mut self) -> Result<(), &'static str> {
        let mut reward = RewardState::default();
        for _ in 0..2 {
            let mut pools: Vec<_> = [crate::character::Character::Defect, crate::character::Character::Ironclad, crate::character::Character::Necrobinder, crate::character::Character::Regent, crate::character::Character::Silent]
                .into_iter().filter(|&pool| pool != self.character).collect();
            self.run_ctx_mut().rngs.niche.shuffle(&mut pools);
            let mut cards = ArrayVec::new();
            for pool in pools.into_iter().take(3) {
                let rarity = self.roll_base_card_rarity();
                #[cfg(feature="selector-proof")]
                if self.options.int1_training_reward_pools {
                    let ids=crate::cardgen::character_pool(pool.into(),false,None,Some(rarity))
                        .ok_or("int1 Kaleidoscope combat pool missing")?.to_vec();
                    cards.push(self.generate_card_from_pool(&ids,rarity,&[],true)?);
                    continue;
                }
                cards.push(self.generate_noncombat_card(pool.into(),rarity,&[],true)?);
            }
            self.apply_card_reward_relic_hooks(&mut cards, false);
            reward.items.push(RewardItem {
                kind: RewardItemKind::Card { cards, closed: false,
                    reroll: Some(CardRewardRerollOptions::EmptyPool), progress: Default::default() },
                consumed: false, skippable: true, skip_completes: false,
                add_injury_after: false, neows_bones_final: false,
            });
        }
        self.reward = reward;
        self.advance_rewards();
        Ok(())
    }

    pub(super) fn offer_neows_bones(&mut self) -> Result<(), &'static str> {
        // Fully unlocked solo Neow profile. MassiveScroll.IsAllowed requires MP;
        // other initial-floor IsAllowed/IsAllowedAtNeow predicates are satisfied.
        if self.is_multiplayer() {
            return Err("NeowsBones multiplayer run ownership is unported");
        }
        let mut relics: Vec<_> = NEOW_RELICS.iter().copied()
            .filter(|id| !matches!(id, RelicId::NeowsBones | RelicId::MassiveScroll))
            .collect();
        self.run_ctx_mut().rngs.rewards.shuffle(&mut relics);
        self.reward = RewardState::default();
        for (index, id) in relics.into_iter().take(2).enumerate() {
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Relic(id), consumed: false, skippable: false,
                skip_completes: true, add_injury_after: false, neows_bones_final: index == 1,
            });
        }
        self.advance_rewards();
        Ok(())
    }

    pub(super) fn add_neows_bones_curse(&mut self) -> Result<(), &'static str> {
        let mut curses: Vec<_> = super::super::card_pools::cards(CardPool::Curse, self.is_multiplayer())?
            .into_iter().filter(|&id| card_info(id, false).is_some_and(|c| c.gen_modifiers))
            .collect();
        curses.sort_by_key(|id| id.name());
        if curses.is_empty() { return Err("NeowsBones curse pool empty"); }
        let index = self.run_ctx_mut().rngs.niche.choice_index(curses.len());
        self.add_card_to_deck(curses[index])
    }
}
