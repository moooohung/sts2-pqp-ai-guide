//! Simulator commands and identities live in a sidecar, outside learned inputs.
use crate::{ActionKind, ContractError, EventOptionId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecisionId {
    pub episode: u64,
    /// Advance after every mutation, including selection toggles and automatic work.
    pub generation: u64,
    /// Absolute authenticated seat; zero is a real seat.
    pub actor: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionSpan { pub start: u16, pub len: u16 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionItem {
    pub option_index: u32,
    /// Instance zero is not used as an absence sentinel.
    pub instance_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MerchantCommand {
    BuyCard { index: u32 },
    BuyRelic { index: u32 },
    BuyPotion { index: u32 },
    RemoveCard { instance: Option<u32> },
}

/// Creature and player targets cannot accidentally coexist in one potion command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PotionRecipient { None, Creature(u16), Player(u8) }

/// Typed replacement for RunAction: each command carries only its own arguments.
/// A card play cannot omit its hand index:
/// ```compile_fail
/// use sts2_native_obs::SoloCommand;
/// let command = SoloCommand::PlayCard { target: None };
/// ```
/// A parameterless command cannot carry an unrelated hand index:
/// ```compile_fail
/// use sts2_native_obs::SoloCommand;
/// let command = SoloCommand::EndTurn { hand: 0 };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloCommand {
    AbandonConfirmation { confirm: bool },
    EventChoice { option: EventOptionId },
    /// Dynamic original-game option templates retain their coordinate payload.
    EventChoiceAt { option: EventOptionId, x: i32, y: i32 },
    Move { x: i32, y: i32 },
    LeaveShop,
    BuyCard { index: u32 },
    BuyRelic { index: u32 },
    BuyPotion { index: u32 },
    /// Opens the removal picker; the subsequent Choose command selects a card.
    RemoveCard,
    RestOption { index: u32 },
    EndTurn,
    PlayCard { hand: u32, target: Option<u16> },
    UsePotion { slot: u32, target: PotionRecipient },
    DiscardPotion { slot: u32 },
    Choose { index: u32 },
    ConfirmChoice,
    Skip,
    PickCard { index: u32 },
    PickSpecialCard { reward_index: u32 },
    RerollCardReward,
    SacrificeCardReward,
    PickCardBundle { index: u32 },
    SkipRewards,
    SkipPotion,
    /// None selects the currently open reward, as in RunManager::pick_potion.
    PickPotion { reward_index: Option<u32> },
    SkipRelic,
    PickRelicReward { reward_index: Option<u32> },
    OpenReward { reward_index: u32 },
    Collect,
}
impl SoloCommand {
    pub const fn action(self) -> ActionKind {
        use ActionKind as A;
        match self {
            Self::AbandonConfirmation { confirm } => if confirm { A::ConfirmChoice } else { A::Skip },
            Self::EventChoice { .. } | Self::EventChoiceAt { .. } => A::EventChoice,
            Self::Move { .. } => A::Move,
            Self::LeaveShop => A::LeaveShop,
            Self::BuyCard { .. } => A::BuyCard,
            Self::BuyRelic { .. } => A::BuyRelic,
            Self::BuyPotion { .. } => A::BuyPotion,
            Self::RemoveCard => A::RemoveCard,
            Self::RestOption { .. } => A::RestOption,
            Self::EndTurn => A::EndTurn,
            Self::PlayCard { .. } => A::PlayCard,
            Self::UsePotion { .. } => A::UsePotion,
            Self::DiscardPotion { .. } => A::DiscardPotion,
            Self::Choose { .. } => A::Choose,
            Self::ConfirmChoice => A::ConfirmChoice,
            Self::Skip => A::Skip,
            Self::PickCard { .. } => A::PickCard,
            Self::PickSpecialCard { .. } => A::PickSpecialCard,
            Self::RerollCardReward => A::RerollCardReward,
            Self::SacrificeCardReward => A::SacrificeCardReward,
            Self::PickCardBundle { .. } => A::PickCardBundle,
            Self::SkipRewards => A::SkipRewards,
            Self::SkipPotion => A::SkipPotion,
            Self::PickPotion { .. } => A::PickPotion,
            Self::SkipRelic => A::SkipRelic,
            Self::PickRelicReward { .. } => A::PickRelicReward,
            Self::OpenReward { .. } => A::OpenReward,
            Self::Collect => A::Collect,
        }
    }
}

/// Variants distinguish real MpRun entry points even when their policy kind agrees.
/// Selection spans are owned by the current observation, not an allocating Vec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Command {
    #[default]
    Invalid,
    Solo(SoloCommand),
    /// A caller-local selection toggle. Dispatch advances the same generation.
    ToggleSelection { index: u32 },
    MpBeginRemoval,
    MpCancelRemoval,
    MpChooseNeow { option: u32 },
    MpSelectNeowCard { index: Option<u32> },
    MpSelectNeowPotion { accept: bool },
    MpMove { x: i32, y: i32 },
    MpPlayCard { hand: u32, target: Option<u16> },
    MpUsePotion { slot: u32, target: Option<u16> },
    MpDiscardPotion { slot: u32 },
    MpEndTurn,
    MpReadyForEnemyTurn,
    MpResolveCombatChoice { index: Option<u32> },
    MpTakeCard { index: u32 },
    MpTakeSpecialCard { reward_index: u32 },
    MpTakePotion,
    MpTakeExtraPotion { index: u32 },
    MpTakeExtraRelic { index: u32 },
    MpTakeGoldSkipRemaining,
    MpMerchant(MerchantCommand),
    MpMerchantPickup { index: Option<u32> },
    MpMerchantPickupCards { selection: SelectionSpan },
    MpLeaveShop,
    MpEventOption { option: u32 },
    MpAbandonConfirmation { option: u32, confirm: bool },
    MpSphereReward { option: u32, choice: SphereRewardChoice },
    MpEventCards { selection: SelectionSpan },
    MpEventPotion { index: u32 },
    MpFinishEventPotions,
    MpEventRelic { accept: bool },
    MpWhisperingReward { index: u32 },
    MpFinishWhisperingOption,
    MpBridgeOption { option: u32 },
    MpCheeseOption { option: u32 },
    MpCheeseCards { indices: [u32; 2] },
    MpScrollBoxBundle { index: u32 },
    MpPomanderCard { instance: u32 },
    MpPreciseScissorsCard { instance: u32 },
    MpNewLeafCard { instance: u32 },
    MpPaelsGrowthCard { instance: u32 },
    MpPrecariousShearsCards { selection: SelectionSpan },
    MpRelicReward { accept: bool },
    MpRestOption { index: u32 },
    MpRestSmith { instance: Option<u32> },
    MpRestMend { target: Option<u8> },
    MpRestCardReward { index: Option<u32> },
    MpRestPotionReward { accept: bool },
    MpSkipRest,
    MpOpenTreasure,
    MpVoteTreasure { index: Option<u32> },
}

impl Command {
    pub fn action(self) -> ActionKind {
        use ActionKind as A;
        match self {
            Self::Invalid => A::Absent,
            Self::Solo(c) => c.action(),
            Self::ToggleSelection { .. } => A::Choose,
            Self::MpBeginRemoval => A::BeginRemoval,
            Self::MpCancelRemoval => A::CancelRemoval,
            Self::MpChooseNeow { .. } => A::ChooseNeow,
            Self::MpMove { .. } => A::Move,
            Self::MpPlayCard { .. } => A::PlayCard,
            Self::MpUsePotion { .. } => A::UsePotion,
            Self::MpDiscardPotion { .. } => A::DiscardPotion,
            Self::MpEndTurn => A::EndTurn,
            Self::MpReadyForEnemyTurn => A::ReadyForEnemyTurn,
            Self::MpTakeCard { .. } => A::PickCard,
            Self::MpTakeSpecialCard { .. } => A::PickSpecialCard,
            Self::MpTakePotion | Self::MpTakeExtraPotion { .. } | Self::MpEventPotion { .. } => A::PickPotion,
            Self::MpTakeExtraRelic { .. } => A::PickRelicReward,
            Self::MpTakeGoldSkipRemaining => A::TakeGoldSkipRemaining,
            Self::MpMerchant(MerchantCommand::BuyCard { .. }) => A::BuyCard,
            Self::MpMerchant(MerchantCommand::BuyRelic { .. }) => A::BuyRelic,
            Self::MpMerchant(MerchantCommand::BuyPotion { .. }) => A::BuyPotion,
            Self::MpMerchant(MerchantCommand::RemoveCard { instance: Some(_) }) => A::RemoveCard,
            Self::MpMerchant(MerchantCommand::RemoveCard { instance: None }) => A::BeginRemoval,
            Self::MpLeaveShop => A::LeaveShop,
            Self::MpAbandonConfirmation { confirm, .. } => if confirm { A::ConfirmChoice } else { A::Skip },
            Self::MpSphereReward { choice, .. } => match choice {
                SphereRewardChoice::Card => A::PickCard,
                SphereRewardChoice::Potion => A::PickPotion,
                SphereRewardChoice::Relic => A::PickRelicReward,
                SphereRewardChoice::Skip => A::Skip,
                SphereRewardChoice::Reroll => A::RerollCardReward,
                SphereRewardChoice::Sacrifice => A::SacrificeCardReward,
            },
            Self::MpEventOption { .. } | Self::MpBridgeOption { .. } | Self::MpCheeseOption { .. } => A::EventChoice,
            Self::MpEventCards { .. } | Self::MpPrecariousShearsCards { .. } | Self::MpCheeseCards { .. }
                | Self::MpMerchantPickupCards { .. } => A::ConfirmChoice,
            Self::MpMerchantPickup { index } => if index.is_some() { A::Choose } else { A::Skip },
            Self::MpFinishEventPotions => A::SkipPotion,
            Self::MpFinishWhisperingOption => A::SkipRewards,
            Self::MpWhisperingReward { .. } => A::Collect,
            Self::MpScrollBoxBundle { .. } => A::PickCardBundle,
            Self::MpPomanderCard { .. } | Self::MpPreciseScissorsCard { .. } | Self::MpNewLeafCard { .. }
                | Self::MpPaelsGrowthCard { .. } => A::Choose,
            Self::MpRestOption { .. } => A::RestOption,
            Self::MpSkipRest => A::Skip,
            Self::MpOpenTreasure => A::TreasureOpen,
            Self::MpVoteTreasure { index: Some(_) } => A::TreasureVote,
            Self::MpVoteTreasure { index: None } => A::TreasureSkip,
            Self::MpSelectNeowCard { index } | Self::MpRestCardReward { index } =>
                if index.is_some() { A::PickCard } else { A::Skip },
            Self::MpResolveCombatChoice { index } =>
                if index.is_some() { A::Choose } else { A::ConfirmChoice },
            Self::MpRestSmith { instance } => if instance.is_some() { A::Choose } else { A::Skip },
            Self::MpRestMend { target } => if target.is_some() { A::Choose } else { A::Skip },
            Self::MpSelectNeowPotion { accept } | Self::MpRestPotionReward { accept } =>
                if accept { A::PickPotion } else { A::SkipPotion },
            Self::MpEventRelic { accept } | Self::MpRelicReward { accept } =>
                if accept { A::PickRelicReward } else { A::SkipRelic },
        }
    }
    pub const fn selection(self) -> Option<SelectionSpan> {
        match self {
            Self::MpEventCards { selection } | Self::MpPrecariousShearsCards { selection }
                | Self::MpMerchantPickupCards { selection } => Some(selection),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SphereRewardChoice { Card, Potion, Relic, Skip, Reroll, Sacrifice }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Binding {
    pub decision: DecisionId,
    pub net_id: u64,
    pub command: Command,
    pub source_instance: Option<u32>,
    pub target_instance: Option<u32>,
    pub offer_instance: Option<u64>,
}
impl Binding {
    /// Structural validity is checked before publication and again before dispatch.
    /// Live indices, instances, phase and legality still belong to the dispatcher.
    pub fn validate_payload(self) -> Result<(), ContractError> {
        match self.command {
            Command::Invalid => Err(ContractError::InvalidBinding),
            Command::Solo(SoloCommand::PlayCard { .. }) | Command::MpPlayCard { .. } if self.source_instance.is_none() =>
                Err(ContractError::InvalidBinding),
            Command::Solo(SoloCommand::UsePotion { target: PotionRecipient::Player(seat), .. })
                if usize::from(seat) >= crate::MAX_SEATS => Err(ContractError::InvalidBinding),
            _ => Ok(()),
        }
    }
    /// A dispatcher must additionally recheck current simulator legality/instances.
    pub fn validate_owner(self, current: DecisionId, net_id: u64) -> Result<(), ContractError> {
        if self.decision.actor != current.actor || self.net_id != net_id {
            return Err(ContractError::WrongOwner);
        }
        if self.decision.episode != current.episode || self.decision.generation != current.generation {
            return Err(ContractError::StaleBinding);
        }
        self.validate_payload()
    }
}
