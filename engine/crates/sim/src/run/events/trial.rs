//! Trial.cs; the entrant number is cosmetic Chaotic RNG, not the event stream.
use super::{EventOption, EventState};
use super::native_identity::{EventOptionIdentity, EventParameter};
use crate::identity::EventOptionId;

pub(crate) enum Outcome {
    Continue, Abandon, MerchantGuilty, MerchantInnocent,
    NobleGuilty, NobleInnocent, NondescriptGuilty, NondescriptInnocent,
}

fn rejected(event: &mut EventState) {
    event.ints[0] = 1;
    event.options.set_page(crate::identity::EventPageId::TrialReject,super::native_identity::EventParameter::None);
    event.options.replace([
        EventOption::new("accept", "Accept", "Stand in judgment"),
        EventOption::new("double_down", "Double Down", "Abandon run confirmation"),
    ]);
}

pub(crate) fn initialize(event: &mut EventState) {
    event.options.replace([
        EventOption::new("accept", "Accept", "Stand in judgment"),
        EventOption::new("reject", "Reject", ""),
    ]);
}

pub(crate) fn choose(event: &mut EventState, option: EventOptionIdentity) -> Result<Outcome, &'static str> {
    match (event.ints[0], option.id, option.parameter) {
        (0, EventOptionId::TrialInitialAccept, _) |
        (1, EventOptionId::TrialRejectAccept, _) => {
            event.ints[0] = 2 + event.rng.choice_index(3) as i32;
            event.options.set_page(match event.ints[0] {
                2=>crate::identity::EventPageId::TrialMerchant,
                3=>crate::identity::EventPageId::TrialNoble,
                _=>crate::identity::EventPageId::TrialNondescript,
            },super::native_identity::EventParameter::None);
            event.options.replace([
                EventOption::new("guilty", "Guilty", ""),
                EventOption::new("innocent", "Innocent", ""),
            ]);
            Ok(Outcome::Continue)
        }
        (0, EventOptionId::TrialInitialReject, _) |
        (5, EventOptionId::TrialRejectDoubleDown, EventParameter::AbandonRunConfirmation(false)) => { rejected(event); Ok(Outcome::Continue) }
        (1, EventOptionId::TrialRejectDoubleDown, EventParameter::None) => {
            // DoubleDown opens NAbandonRunConfirmPopup; cancelling leaves the
            // rejection page unchanged. It does not kill the owner's creature.
            event.ints[0] = 5;
            event.options.set_page(crate::identity::EventPageId::TrialReject,
                super::native_identity::EventParameter::AbandonRunConfirmation(false));
            event.options.replace([
                EventOption::new("confirm_abandon", "Confirm", "Abandon run"),
                EventOption::new("cancel_abandon", "Cancel", "Return to event"),
            ]);
            Ok(Outcome::Continue)
        }
        (5, EventOptionId::TrialRejectDoubleDown, EventParameter::AbandonRunConfirmation(true)) => Ok(Outcome::Abandon),
        (2, EventOptionId::TrialMerchantGuilty, _) => Ok(Outcome::MerchantGuilty),
        (2, EventOptionId::TrialMerchantInnocent, _) => Ok(Outcome::MerchantInnocent),
        (3, EventOptionId::TrialNobleGuilty, _) => Ok(Outcome::NobleGuilty),
        (3, EventOptionId::TrialNobleInnocent, _) => Ok(Outcome::NobleInnocent),
        (4, EventOptionId::TrialNondescriptGuilty, _) => Ok(Outcome::NondescriptGuilty),
        (4, EventOptionId::TrialNondescriptInnocent, _) => Ok(Outcome::NondescriptInnocent),
        _ => Err("invalid Trial option"),
    }
}
