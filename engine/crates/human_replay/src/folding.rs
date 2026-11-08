//! Original C# reward flows → existing native actions, not a new action space.
//! Audited game 0861bfa1: Rewards/CardReward.cs::OnSelect opens a selector and
//! awaits its choice; Reward.cs::SelectUnsynchronized then removes the reward.
//! GoldReward.cs::OnSelect calls GainGold once per reward. These rules identify
//! candidate folds only: callers must still replay and compare the resulting
//! native-boundary checkpoint before admitting a sample.
use sts2_native_obs::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardIdentity {
    pub id: String,
    pub upgrade: u32,
}
impl CardIdentity {
    /// CardInfo.entry is the original ModelId.Entry. Native/r7 enum names
    /// disambiguate power-colliding classes with `_CARD` (e.g. JugglingCard);
    /// those internal aliases must not become checkpoint divergences.
    pub fn from_sim(card: &sts2_sim::card::Card) -> Result<Self,FoldError> {
        let info=sts2_sim::tables::card_info(card.id,card.upgraded)
            .ok_or(FoldError::UnknownCardIdentity)?;
        Ok(Self {id:info.entry.to_owned(),upgrade:u32::from(card.upgraded)})
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoldError {
    MissingChoice,
    SeatMismatch,
    ParentMismatch,
    OfferedIdentityMismatch,
    PartialGoldCollection,
    GoldOrderMismatch,
    MissingLeaveBoundary,
    InvalidForcedSelector,
    UnknownCardIdentity,
}
impl FoldError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingChoice => "FOLD_MISSING_REWARD_CHOICE",
            Self::SeatMismatch => "FOLD_SEAT_MISMATCH",
            Self::ParentMismatch => "FOLD_PARENT_MISMATCH",
            Self::OfferedIdentityMismatch => "FOLD_OFFER_IDENTITY_MISMATCH",
            Self::PartialGoldCollection => "FOLD_PARTIAL_GOLD_COLLECTION",
            Self::GoldOrderMismatch => "FOLD_GOLD_ORDER_MISMATCH",
            Self::MissingLeaveBoundary => "FOLD_MISSING_REWARD_LEAVE_BOUNDARY",
            Self::InvalidForcedSelector => "FOLD_NOT_FORCED_SELECTOR",
            Self::UnknownCardIdentity => "FOLD_UNKNOWN_CARD_IDENTITY",
        }
    }
}

/// Cards/Headbutt.cs::OnPlay selects one from Discard. Commands/CardSelectCmd.cs
/// ::FromCombatPile returns the full offered list automatically for 0 cards,
/// or for <= MinSelect with no manual confirmation. Headbutt's minimum is 1.
/// Such a forced selector child folds into its parent PlayCard, but only if
/// the recorded result is exactly that offered list (including identities).
pub fn fold_forced_headbutt(discard: &[CardIdentity], selected: &[CardIdentity])
    -> Result<(),FoldError>
{
    if discard.len()<=1 && selected==discard { Ok(()) }
    else { Err(FoldError::InvalidForcedSelector) }
}

/// Selector data captured by the game, never inferred from a deck delta or
/// from absence of a child row (historical remote-seat v1 rows omit choices).
pub struct CardRewardChoice {
    pub seq: u32,
    pub parent_seq: u32,
    pub seat: u8,
    pub index: u32,
    pub offered: Vec<CardIdentity>,
    pub chosen: CardIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewardFold {
    pub seat: u8,
    pub command: Command,
    pub consumed_sequences: Vec<u32>,
    /// Async parent completion is AFTER its child selector. Compare the
    /// parent's after checkpoint, not the child's pre-acquisition checkpoint.
    pub checkpoint_seq: u32,
}

/// OpenReward(card) + its authenticated PickCard child → MpTakeCard(index).
/// No mutable inventory, RNG, or other original input may be repaired here.
pub fn fold_card_reward(open_seq: u32, seat: u8, choice: Option<&CardRewardChoice>)
    -> Result<RewardFold, FoldError>
{
    let choice = choice.ok_or(FoldError::MissingChoice)?;
    if choice.seat != seat { return Err(FoldError::SeatMismatch); }
    if choice.parent_seq != open_seq || choice.seq <= open_seq {
        return Err(FoldError::ParentMismatch);
    }
    if choice.offered.get(choice.index as usize) != Some(&choice.chosen) {
        return Err(FoldError::OfferedIdentityMismatch);
    }
    Ok(RewardFold { seat, command: Command::MpTakeCard { index: choice.index },
        consumed_sequences: vec![open_seq, choice.seq], checkpoint_seq: open_seq })
}

pub struct GoldCollection {
    pub seq: u32,
    pub seat: u8,
    pub amount: i32,
}

/// Individual GoldReward selections followed by leaving the reward screen →
/// MpTakeGoldSkipRemaining. The native implementation applies GainGold once
/// per entry in its stored order, then completes this seat's rewards.
/// A prefix, reordered collection, or still-open menu has no exact fold.
pub fn fold_gold_tail(seat: u8, offered: &[i32], collected: &[GoldCollection],
    leave_seq: Option<u32>) -> Result<RewardFold, FoldError>
{
    let checkpoint_seq = leave_seq.ok_or(FoldError::MissingLeaveBoundary)?;
    if offered.len() != collected.len() { return Err(FoldError::PartialGoldCollection); }
    let mut consumed_sequences = Vec::with_capacity(collected.len());
    for (index, (amount, input)) in offered.iter().zip(collected).enumerate() {
        if input.seat != seat { return Err(FoldError::SeatMismatch); }
        if amount != &input.amount { return Err(FoldError::GoldOrderMismatch); }
        if input.seq > checkpoint_seq || index > 0 && input.seq <= collected[index-1].seq {
            return Err(FoldError::GoldOrderMismatch);
        }
        consumed_sequences.push(input.seq);
    }
    Ok(RewardFold { seat, command: Command::MpTakeGoldSkipRemaining,
        consumed_sequences, checkpoint_seq })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gold_requires_complete_ordered_collections_and_a_leave_boundary() {
        let inputs = [GoldCollection { seq: 4, seat: 1, amount: 13 },
            GoldCollection { seq: 6, seat: 1, amount: 15 }];
        assert_eq!(fold_gold_tail(1, &[13,15], &inputs, Some(7)).unwrap().command,
            Command::MpTakeGoldSkipRemaining);
        assert_eq!(fold_gold_tail(1, &[13,15], &inputs[..1], Some(7)), Err(FoldError::PartialGoldCollection));
        assert_eq!(fold_gold_tail(1, &[15,13], &inputs, Some(7)), Err(FoldError::GoldOrderMismatch));
        assert_eq!(fold_gold_tail(1, &[13,15], &inputs, None), Err(FoldError::MissingLeaveBoundary));
        assert_eq!(fold_gold_tail(0, &[13,15], &inputs, Some(7)), Err(FoldError::SeatMismatch));
    }
    #[test]
    fn forced_headbutt_does_not_suppress_a_real_selection() {
        assert_eq!(fold_forced_headbutt(&[],&[]),Ok(()));
        let card=CardIdentity{id:"STRIKE_IRONCLAD".into(),upgrade:0};
        assert_eq!(fold_forced_headbutt(&[card.clone()],&[card.clone()]),Ok(()));
        assert_eq!(fold_forced_headbutt(&[card],&[]),Err(FoldError::InvalidForcedSelector));
    }
}
