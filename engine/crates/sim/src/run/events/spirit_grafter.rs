use super::EventEffect;
use crate::tables::CardId;

pub(crate) const REJECTION_DAMAGE: i32 = 10;

pub(crate) fn accept() -> [EventEffect; 2] {
    [EventEffect::Heal(25), EventEffect::AddCard(CardId::Metamorphosis)]
}
