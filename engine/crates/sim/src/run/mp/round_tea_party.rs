use super::MpRun;
use crate::{identity::EventOptionId, run::events::{EventEffect, EventOption, EventOptionIdentity, EventState}};
use crate::tables::RelicId;

impl MpRun {
    pub(super) fn choose_round_tea_party(&mut self, seat: usize, mut event: EventState,
        option: EventOptionIdentity) -> Result<(), &'static str> {
        match option.id {
            EventOptionId::RoundTeaPartyInitialPickFight if event.ints[0] == 0 => {
                event.ints[0] = 1;
                event.options.set_page(crate::identity::EventPageId::RoundTeaPartyPickFight,
                    crate::run::events::native_identity::EventParameter::None);
                event.options.replace([EventOption::new("continue_fight", "Continue Fight", "")]);
                self.events[seat] = Some(event);
                return Ok(());
            }
            EventOptionId::RoundTeaPartyInitialEnjoyTea if event.ints[0] == 0 => {
                self.attach_event_relic(seat, RelicId::RoyalPoison)?;
                let max_hp = self.roster.player(seat).unwrap().max_hp;
                self.apply_event_effect(seat, EventEffect::Heal(max_hp), &mut event)?;
            }
            EventOptionId::RoundTeaPartyPickFightContinueFight if event.ints[0] == 1 => {
                self.apply_event_effect(seat, EventEffect::LoseHp(11), &mut event)?;
                let relic = self.pull_event_relic_from_front(seat)?;
                self.attach_event_relic(seat, relic)?;
            }
            _ => return Err("invalid Round Tea Party option"),
        }
        self.finish_event_seat(seat);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Character, MpPhase, PlayerSpec};
    #[test]
    fn tea_is_owner_bound_and_fight_waits_for_continue() {
        let mut run = MpRun::new("73003", &[
            PlayerSpec { net_id: 7, character: Character::Ironclad },
            PlayerSpec { net_id: 42, character: Character::Silent },
        ], 0).unwrap();
        run.phase = MpPhase::Event;
        run.begin_generic_event(crate::identity::EventId::RoundTeaParty).unwrap();
        run.roster.player_mut(0).unwrap().hp = 20;
        run.roster.player_mut(1).unwrap().hp = 30;
        let rng = run.roster.player(1).unwrap().rngs.rewards.counter;
        assert!(run.choose_event_option(7, 1, 1).is_err());
        run.choose_event_option(42, 1, 1).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, 30);
        assert_eq!(run.roster.player(1).unwrap().relics.len(), 1);
        assert_eq!(run.roster.player(1).unwrap().rngs.rewards.counter, rng);
        run.choose_event_option(42, 1, 0).unwrap();
        assert_eq!(run.roster.player(1).unwrap().hp, 19);
        assert_eq!(run.roster.player(1).unwrap().relics.len(), 2);
        assert!(!run.event_done[0]);
        run.choose_event_option(7, 0, 0).unwrap();
        assert_eq!(run.roster.player(0).unwrap().hp, 80);
        assert!(run.roster.player(0).unwrap().relics.iter().any(|r| r.id == RelicId::RoyalPoison));
        assert_eq!(run.roster.player(1).unwrap().hp, 19);
    }
}
