use super::{MpPhase, MpRun};
use crate::{card::enchant::apply_static_enchantment, relic::Relic, tables::RelicId};
use super::super::manager::RunManager;

impl MpRun {
    /// PaelsGrowth.AfterObtained and CardSelectCmd.FromDeckForEnchantment:
    /// deck order, Clone eligibility, mandatory one-card selection; amount 4.
    pub fn obtain_paels_growth(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Map { return Err("pickup requires map phase"); }
        let mut next = self.clone();
        next.begin_paels_growth(seat)?;
        *self = next;
        Ok(())
    }

    pub(super) fn begin_paels_growth(&mut self, seat: usize) -> Result<(), &'static str> {
        if self.owner_pickup_pending(seat) { return Err("owner pickup choice pending"); }
        let player = self.roster.player_mut(seat).ok_or("invalid pickup owner")?;
        let choices: Vec<_> = player.deck.iter()
            .filter(|card| RunManager::event_can_enchant(card, "Clone"))
            .map(|card| card.instance_id).collect();
        let mut relic = Relic::new(RelicId::PaelsGrowth);
        relic.owner = seat as u8;
        relic.floor_added_to_deck = self.world.run.total_floor;
        player.relics.push(relic);
        // C# selects all automatically when candidate count <= MinSelect.
        if choices.len() == 1 {
            let card = player.deck.iter_mut().find(|c| c.instance_id == choices[0]).unwrap();
            if !apply_static_enchantment(card, "Clone", 4) { return Err("Clone enchantment capacity exceeded"); }
        } else if choices.len() > 1 {
            self.paels_growth_choices[seat] = Some(choices);
        }
        self.record_relic_seen(seat, RelicId::PaelsGrowth);
        Ok(())
    }

    pub fn select_paels_growth_card(&mut self, net_id: u64, seat: usize, instance_id: u32)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map) { return Err("not at inventory choice"); }
        let choices = self.paels_growth_choices[seat].as_ref().ok_or("no owner Clone choice")?;
        if !choices.contains(&instance_id) { return Err("invalid owner Clone choice"); }
        let card = self.roster.player_mut(seat).unwrap().deck.iter_mut()
            .find(|c| c.instance_id == instance_id).ok_or("Clone candidate missing")?;
        if !RunManager::event_can_enchant(card, "Clone") { return Err("Clone candidate no longer eligible"); }
        if !apply_static_enchantment(card, "Clone", 4) { return Err("Clone enchantment capacity exceeded"); }
        self.paels_growth_choices[seat] = None;
        if self.phase == MpPhase::Neow { self.advance_neow(); }
        Ok(())
    }
}
