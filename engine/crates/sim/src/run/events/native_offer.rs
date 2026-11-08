//! Only retained content actually shown by an indexed event option.
use super::*;
use crate::{run::mp::Character, tables::{PotionId, CardType}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEventOffer {
    Relic(RelicId),
    Pool(Character),
    Potion(PotionId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::EventId;

    #[test]
    fn indexed_offers_resolve_retained_content_not_slot_names() {
        for event in [EventId::Darv, EventId::Nonupeipe, EventId::Orobas,
            EventId::Pael, EventId::Vakuu, EventId::Tezcatara, EventId::DollRoom,
            EventId::FakeMerchant] {
            let mut state = EventState::new(event, Rng::new(7));
            state.relics.push(RelicId::HappyFlower);
            let option = native_identity::EventOptionIdentity {
                id: crate::identity::EventOptionId::ALL.into_iter()
                    .find(|id| id.event() == event).unwrap(),
                parameter: EventParameter::Offer(0),
            };
            assert_eq!(state.native_offer(option, &[]), Some(NativeEventOffer::Relic(RelicId::HappyFlower)));
            state.relics[0] = RelicId::Girya;
            assert_eq!(state.native_offer(option, &[]), Some(NativeEventOffer::Relic(RelicId::Girya)));
        }
        let mut state = EventState::new(EventId::ColorfulPhilosophers, Rng::new(7));
        let mut option = native_identity::EventOptionIdentity {
            id: crate::identity::EventOptionId::ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant,
            parameter: EventParameter::Offer(0),
        };
        state.ints[1] = 0;
        assert_eq!(state.native_offer(option, &[]), Some(NativeEventOffer::Pool(Character::Necrobinder)));
        state.ints[1] = 4;
        assert_eq!(state.native_offer(option, &[]), Some(NativeEventOffer::Pool(Character::Defect)));
        state = EventState::new(EventId::TheFutureOfPotions, Rng::new(7));
        option.id = crate::identity::EventOptionId::TheFutureOfPotionsInitialPotion;
        assert_eq!(state.native_offer(option, &[Some(PotionId::Firepotion)]),
            Some(NativeEventOffer::Potion(PotionId::Firepotion)));
        assert_eq!(state.native_offer(option, &[Some(PotionId::Blockpotion)]),
            Some(NativeEventOffer::Potion(PotionId::Blockpotion)));
    }
}

impl EventState {
    /// Locked Wongo options show only the price, with no item name or hover.
    pub(crate) fn public_wongo_featured(&self) -> Option<RelicId> {
        if self.id!=crate::identity::EventId::WelcomeToWongos
            || !self.options.iter().any(|option|option.enabled
                && option.option_id.id==crate::identity::EventOptionId::WelcomeToWongosInitialFeaturedItem) {
            return None;
        }
        self.relics.first().copied()
    }

    pub(crate) fn native_offer(&self, option: native_identity::EventOptionIdentity,
        potions: &[Option<PotionId>]) -> Option<NativeEventOffer> {
        use crate::identity::EventId as E;
        use NativeEventOffer as O;
        match option.parameter {
            EventParameter::Relic(id) => Some(O::Relic(id)),
            EventParameter::Offer(slot) => {
                let slot = usize::from(slot);
                match self.id {
                    E::Darv | E::Nonupeipe | E::Orobas | E::Pael | E::Vakuu |
                    E::Tanx | E::DollRoom | E::FakeMerchant =>
                        self.relics.get(slot).copied().map(O::Relic),
                    // Transport ordinals 3..8 distinguish the Toy Box reward
                    // callbacks from Tezcatara's initial three callbacks.
                    // The retained reward inventory itself is still slot 0..4.
                    E::Tezcatara => match slot {
                        0..=2 => self.relics.get(slot).copied().map(O::Relic),
                        3..=7 => self.options.iter().position(|entry| entry.option_id == option)
                            .and_then(|index| self.relics.get(index)).copied().map(O::Relic),
                        _ => None, // skip remaining wax rewards
                    },
                    E::ColorfulPhilosophers => {
                        const POOLS: [Character; 5] = [Character::Necrobinder, Character::Ironclad,
                            Character::Regent, Character::Silent, Character::Defect];
                        POOLS.get(usize::try_from(*self.ints.get(slot + 1)?).ok()?).copied().map(O::Pool)
                    }
                    E::TheFutureOfPotions => potions.get((*self.ints.get(slot)? >> 2) as usize)
                        .copied().flatten().map(O::Potion),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Dusty Tome hover card and Future of Potions reward type are public,
    /// but other retained cards and future reward rolls are not.
    pub(crate) fn native_offer_detail(&self, option: native_identity::EventOptionIdentity,
        potions: &[Option<PotionId>]) -> (Option<CardId>, Option<CardType>) {
        match self.native_offer(option, potions) {
            Some(NativeEventOffer::Relic(RelicId::DustyTome)) =>
                (self.cards.first().copied(), None),
            Some(NativeEventOffer::Potion(_)) => {
                let EventParameter::Offer(slot) = option.parameter else { unreachable!() };
                (None, [CardType::Attack, CardType::Skill, CardType::Power]
                    .get((self.ints[usize::from(slot)] & 3) as usize).copied())
            }
            _ => (None, None),
        }
    }
}
