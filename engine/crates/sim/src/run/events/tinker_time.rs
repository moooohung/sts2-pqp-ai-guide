//! TinkerTime.cs: full UnstableShuffle/Take(2) at both choice pages.
use super::{EventOption, EventState};
use crate::{card::Card, identity::EventOptionId, tables::{CardId, CardType, TargetType, VarKey}};
use super::native_identity::EventOptionIdentity;

pub(crate) enum Step { Types, Riders, Complete }

const TYPES: &[(EventOptionId, &str)] = &[
    (EventOptionId::TinkerTimeChooseCardTypeAttack, "Attack"),
    (EventOptionId::TinkerTimeChooseCardTypeSkill, "Skill"),
    (EventOptionId::TinkerTimeChooseCardTypePower, "Power")];
const RIDERS: &[(EventOptionId, &str)] = &[
    (EventOptionId::TinkerTimeChooseRiderSapping, "Sapping"),
    (EventOptionId::TinkerTimeChooseRiderViolence, "Violence"),
    (EventOptionId::TinkerTimeChooseRiderChoking, "Choking"),
    (EventOptionId::TinkerTimeChooseRiderEnergized, "Energized"),
    (EventOptionId::TinkerTimeChooseRiderWisdom, "Wisdom"),
    (EventOptionId::TinkerTimeChooseRiderChaos, "Chaos"),
    (EventOptionId::TinkerTimeChooseRiderExpertise, "Expertise"),
    (EventOptionId::TinkerTimeChooseRiderCurious, "Curious"),
    (EventOptionId::TinkerTimeChooseRiderImprovement, "Improvement"),
];

pub(crate) fn initialize(event: &mut EventState) {
    event.options.push(EventOption::new("choose_card_type", "Choose Card Type", ""));
}

pub(crate) fn choose(event: &mut EventState, option: EventOptionIdentity) -> Result<Step, &'static str> {
    if event.ints[0] == 0 && option.is(EventOptionId::TinkerTimeInitialChooseCardType) {
        let mut types: Vec<_> = TYPES.iter().copied().collect();
        event.rng.shuffle(&mut types);
        event.options.set_page(crate::identity::EventPageId::TinkerTimeChooseCardType, super::native_identity::EventParameter::None);
        event.options.replace_ids(types.into_iter().take(2)
            .map(|(id, label)| (id, label, "Create Mad Science")));
        event.ints[0] = 1;
        return Ok(Step::Types);
    }
    if event.ints[0] == 1 {
        let kind = TYPES.iter().position(|(id, _)| *id == option.id).ok_or("invalid TinkerTime type")?;
        event.ints[1] = kind as i32;
        let mut riders: Vec<_> = RIDERS[kind * 3..kind * 3 + 3].iter().copied().collect();
        event.rng.shuffle(&mut riders);
        event.options.set_page(crate::identity::EventPageId::TinkerTimeChooseRider, super::native_identity::EventParameter::None);
        event.options.replace_ids(riders.into_iter().take(2)
            .map(|(id, label)| (id, label, "Add Mad Science to your deck")));
        event.ints[0] = 2;
        return Ok(Step::Riders);
    }
    if event.ints[0] == 2 {
        event.ints[2] = RIDERS.iter().position(|(id, _)| *id == option.id)
            .ok_or("invalid TinkerTime rider")? as i32 + 1;
        return Ok(Step::Complete);
    }
    Err("invalid TinkerTime page")
}

pub(crate) fn card(event: &EventState, instance: u32) -> Result<Card, &'static str> {
    let mut card = Card::from_factory(CardId::MadScience, false, instance).ok_or("MadScience factory")?;
    card.card_type = match event.ints[1] {
        0 => CardType::Attack, 1 => CardType::Skill, 2 => CardType::Power,
        _ => return Err("invalid TinkerTime type"),
    };
    card.target = if card.card_type == CardType::Attack { TargetType::AnyEnemy } else { TargetType::Self_ };
    card.damage = (card.card_type == CardType::Attack).then_some(12);
    card.block = (card.card_type == CardType::Skill).then_some(8);
    card.vars.clear();
    card.vars.extend([
        (VarKey::Damage, 12), (VarKey::Block, 8),
        (VarKey::SappingWeak, 2), (VarKey::SappingVulnerable, 2),
        (VarKey::ViolenceHits, 3), (VarKey::ChokingDamage, 6),
        (VarKey::EnergizedEnergy, 2), (VarKey::WisdomCards, 3),
        (VarKey::ExpertiseStrength, 2), (VarKey::ExpertiseDexterity, 2),
        (VarKey::CuriousReduction, 1), (VarKey::Rider, event.ints[2]),
    ]);
    Ok(card)
}
