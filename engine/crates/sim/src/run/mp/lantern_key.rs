use super::{MpRun, MpPhase};
use crate::{card::Card, combat::EndReward, identity::EventOptionId, tables::RoomType, run::events::{EventOptionIdentity, lantern_key::{self, Action}}};

impl MpRun {
    // EventSynchronizer.ChooseOptionForSharedEvent applies the resolved host
    // choice to every owner; EventCombatSynchronizer retains each owner's extra.
    pub(super) fn choose_lantern_key(&mut self, option: EventOptionIdentity) -> Result<(), &'static str> {
        for seat in 0..self.roster.players().len() {
            let Some(mut event) = self.events[seat].take() else { continue };
            match lantern_key::choose(&mut event, option)? {
                Action::Return(effect) => {
                    self.apply_event_effect(seat, effect, &mut event)?;
                    self.finish_event_seat(seat);
                }
                Action::Keep => {
                    self.events[seat] = Some(event);
                }
                Action::Fight => {}
            }
        }
        if option.is(EventOptionId::TheLanternKeyKeepTheKeyFight) {
            let mut combat = self.combat.take().ok_or("Lantern Key layout missing")?;
            self.roster.refresh_prepared_combat_inventory(&mut combat);
            for seat in 0..self.roster.players().len() {
                let iid = combat.run.new_card_id();
                let mut card = Card::from_factory(lantern_key::CARD, false, iid).ok_or("Lantern Key card factory missing")?;
                card.owner = seat as u8;
                combat.end_rewards.push(EndReward::SpecialCard { owner: seat as u8, card });
            }
            self.roster.next_card_id = combat.run.next_card_id;
            combat.start_combat();
            if let Some(fault) = combat.fault { return Err(fault); }
            self.combat = Some(combat);
            self.events.fill(None);
            self.event_done.fill(true);
            self.world.current_room = Some(lantern_key::ENCOUNTER.name());
            self.world.run.room_type = RoomType::Monster;
            self.phase = MpPhase::Combat;
        }
        Ok(())
    }
}
