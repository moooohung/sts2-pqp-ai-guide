use super::{MpPhase, MpRun};
use crate::{relic::Relic, tables::{CardId, CardType, PotionId, RelicId, KW_ETERNAL}};

impl MpRun {
    pub(super) fn owner_pickup_pending(&self, seat: usize) -> bool {
        self.paels_growth_choices[seat].is_some() || self.shears_choices[seat].is_some()
            || self.new_leaf_choices[seat].is_some()
            || self.pomander_choices[seat].is_some()
            || self.precise_scissors_choices[seat].is_some()
            || self.relic_rewards[seat].is_some()
            || self.neow_potion_rewards[seat].is_some()
            || self.scroll_box_bundles[seat].is_some()
    }

    pub fn obtain_precarious_shears(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Map { return Err("pickup requires map phase"); }
        let mut next = self.clone();
        next.begin_precarious_shears(seat)?;
        *self = next;
        Ok(())
    }

    pub(super) fn begin_precarious_shears(&mut self, seat: usize) -> Result<(), &'static str> {
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let player = self.roster.player_mut(seat).ok_or("invalid pickup owner")?;
        // CardSelectCmd.FromDeckForRemoval: IsRemovable, curses first, then
        // original deck order. Dead owners return no selected cards.
        let mut candidates: Vec<_> = player.deck.iter().filter(|c|
            player.hp > 0 && !c.has_kw(KW_ETERNAL)).collect();
        candidates.sort_by_key(|c| c.card_type != CardType::Curse);
        let choices: Vec<_> = candidates.iter().map(|c| c.instance_id).collect();
        let mut relic = Relic::new(RelicId::PrecariousShears);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        player.relics.push(relic);
        self.record_relic_seen(seat, RelicId::PrecariousShears);
        if choices.len() <= 2 {
            self.finish_shears(seat, &choices)?;
        } else {
            self.shears_choices[seat] = Some(choices);
        }
        Ok(())
    }

    pub fn select_precarious_shears_cards(&mut self, net_id: u64, seat: usize, selected: &[u32])
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map) { return Err("not at inventory choice"); }
        let choices = self.shears_choices[seat].as_ref().ok_or("no owner removal choice")?;
        if selected.len() != 2 || selected[0] == selected[1]
            || selected.iter().any(|id| !choices.contains(id)) {
            return Err("invalid owner removal selection");
        }
        let mut next = self.clone();
        next.finish_shears(seat, selected)?;
        next.shears_choices[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    fn finish_shears(&mut self, seat: usize, selected: &[u32]) -> Result<(), &'static str> {
        let player = self.roster.player_mut(seat).unwrap();
        for id in selected {
            let card = player.deck.iter().find(|c| c.instance_id == *id)
                .ok_or("removal candidate missing")?;
            if card.has_kw(KW_ETERNAL) { return Err("removal candidate no longer eligible"); }
            if card.id == CardId::SpoilsMap {
                return Err("unported Spoils Map removal callback");
            }
        }
        let damage = (16 - i32::from(player.relics.iter().any(|r|
            r.id == RelicId::TungstenRod && r.enabled))).max(0);
        if player.hp > 0 && damage >= player.hp && (player.relics.iter().any(|r|
            r.id == RelicId::LizardTail && r.enabled && !r.flag1)
            || player.potions.contains(&Some(PotionId::Fairyinabottle))) {
            return Err("unported pickup damage death prevention");
        }
        // PrecariousShears.AfterObtained removes selected cards before dealing
        // 16 unpowered damage to this owner. TungstenRod reduces actual HP loss.
        player.retain_deck_cards(|c| !selected.contains(&c.instance_id));
        player.hp = (player.hp - damage).max(0);
        if self.roster.players().iter().all(|p| p.hp <= 0) { self.phase = MpPhase::RunOver; }
        Ok(())
    }
}
