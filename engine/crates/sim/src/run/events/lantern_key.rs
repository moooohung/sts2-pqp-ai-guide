use super::{EventEffect, EventOption, EventState};
use super::native_identity::EventOptionIdentity;
use crate::identity::{EncounterId, EventOptionId};
use crate::tables::CardId;

pub(crate) const ENCOUNTER: EncounterId = EncounterId::MysteriousKnightEventEncounter;
pub(crate) const CARD: CardId = CardId::LanternKey;

pub(crate) enum Action {
    Return(EventEffect),
    Keep,
    Fight,
}

pub(crate) fn choose(event: &mut EventState, option: EventOptionIdentity) -> Result<Action, &'static str> {
    match option.id {
        EventOptionId::TheLanternKeyInitialReturnTheKey => Ok(Action::Return(EventEffect::GainGold(100))),
        EventOptionId::TheLanternKeyInitialKeepTheKey => {
            event.options.clear();
            event.options.push(EventOption::new("fight", "Fight", "Fight for Lantern Key card"));
            Ok(Action::Keep)
        }
        EventOptionId::TheLanternKeyKeepTheKeyFight => Ok(Action::Fight),
        _ => Err("invalid Lantern Key option"),
    }
}
