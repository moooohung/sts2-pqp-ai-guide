use super::*;
use crate::{identity::EventOptionId, run::events::EventOptionIdentity};

impl MpRun {
    pub(super) fn initialize_relic_trader_event(&mut self, seat: usize, event: &mut EventState)
        -> Result<(), &'static str> {
        let mut owned: Vec<_> = self.roster.player(seat).unwrap().relics.iter().enumerate()
            .filter_map(|(i, r)| Self::tradable_relic(r).then_some(i)).collect();
        owned.sort_by_key(|&i| self.roster.player(seat).unwrap().relics[i].id.name());
        event.rng.shuffle(&mut owned);
        if owned.is_empty() {
            event.options.push(EventOption::new("proceed", "Proceed", ""));
        } else {
            for _ in 0..3 { event.relics.push(self.pull_event_relic_from_front(seat)?); }
            for (i, (&index, option)) in owned.iter().zip(["trade_0", "trade_1", "trade_2"]).enumerate() {
                event.ints[i] = index as i32;
                event.options.push(EventOption::new(option, "Trade Relic", ""));
            }
        }
        Ok(())
    }

    pub(super) fn tradable_relic(r: &Relic) -> bool {
        let info = &RELIC_INFO[r.id.index()];
        !r.is_melted && !info.upon_pickup && !info.spawns_pets
            && !(matches!(r.id, RelicId::MawBank | RelicId::LizardTail) && r.flag1)
            && !matches!(info.rarity, RelicRarity::Starter | RelicRarity::Event | RelicRarity::Ancient)
    }

    pub(super) fn choose_relic_trader_event(&mut self, seat: usize, event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        if !option.is(EventOptionId::RelicTraderProceed) {
            let index = event.options.iter().position(|o| o.option_id == option).ok_or("Trader option missing")?;
            self.roster.player_mut(seat).unwrap().relics.remove(event.ints[index] as usize);
            let id = event.relics[index];
            self.attach_event_relic(seat, id)?;
            self.record_relic_seen(seat, id);
        }
        self.finish_event_seat(seat);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::mp::{Character, PlayerSpec};

    #[test]
    fn trader_retains_three_offers_and_swaps_only_the_owners_selected_relic() {
        let specs = [PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent }];
        let mut run = MpRun::new("73003", &specs, 0).unwrap();
        run.phase = MpPhase::Event;
        run.world.current_room = Some("RELIC_TRADER");
        run.roster.player_mut(1).unwrap().relics.push(Relic::new(RelicId::Anchor));
        let other = format!("{:?}", run.roster.player(0).unwrap());
        let counter = run.roster.player(1).unwrap().rngs.rewards.counter;
        run.begin_generic_event(crate::identity::EventId::RelicTrader).unwrap();
        assert_eq!(run.event_options(42, 1).unwrap(), vec!["trade_0"]);
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, counter + 3);
        let offered = run.events[1].as_ref().unwrap().relics[0];
        run.choose_event_option(42, 1, 0).unwrap();
        let p = run.roster.player(1).unwrap();
        assert!(!p.relics.iter().any(|r| r.id == RelicId::Anchor));
        assert!(p.relics.iter().any(|r| r.id == offered));
        assert_eq!(other, format!("{:?}", run.roster.player(0).unwrap()));
        assert!(run.event_done[1]);
    }
}
