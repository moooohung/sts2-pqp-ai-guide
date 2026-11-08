//! Playable character identity shared by combat, run and multiplayer layers.
//! The C# model id string exists only for ingestion, digests and diagnostics.
use crate::tables::{CardId, RelicId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Character { Ironclad, Silent, Defect, Necrobinder, Regent }

impl Character {
    pub const ALL: [Self; 5] = [Self::Ironclad, Self::Silent, Self::Defect, Self::Necrobinder, Self::Regent];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ironclad => "IRONCLAD", Self::Silent => "SILENT",
            Self::Defect => "DEFECT", Self::Necrobinder => "NECROBINDER",
            Self::Regent => "REGENT",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    // Models/Characters/{Ironclad,Silent,Defect,Necrobinder,Regent}.cs
    // StartingHp, StartingDeck, StartingRelics and BaseOrbSlotCount.
    pub(crate) fn starting(self) -> (i32, usize, CardId, CardId, &'static [CardId], RelicId) {
        use CardId::*;
        match self {
            Self::Ironclad => (80, 5, StrikeIronclad, DefendIronclad, &[Bash], RelicId::BurningBlood),
            Self::Silent => (70, 5, StrikeSilent, DefendSilent, &[Neutralize, Survivor], RelicId::RingOfTheSnake),
            Self::Defect => (75, 4, StrikeDefect, DefendDefect, &[Zap, Dualcast], RelicId::CrackedCore),
            Self::Necrobinder => (66, 4, StrikeNecrobinder, DefendNecrobinder, &[Bodyguard, Unleash], RelicId::BoundPhylactery),
            Self::Regent => (75, 4, StrikeRegent, DefendRegent, &[FallingStar, Venerate], RelicId::DivineRight),
        }
    }
}

impl core::fmt::Display for Character {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(self.name()) }
}

/// C# Models/CardPools/*CardPool: each character's pool plus the shared ones.
/// Combat generation also has a player-count variant of the character and
/// colorless pools, selected by a separate `multiplayer` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardPool { Ironclad, Silent, Defect, Regent, Necrobinder, Colorless, Curse, Status }

impl CardPool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ironclad => "IRONCLAD", Self::Silent => "SILENT", Self::Defect => "DEFECT",
            Self::Regent => "REGENT", Self::Necrobinder => "NECROBINDER",
            Self::Colorless => "COLORLESS", Self::Curse => "CURSE", Self::Status => "STATUS",
        }
    }
}

impl From<Character> for CardPool {
    fn from(character: Character) -> Self {
        match character {
            Character::Ironclad => Self::Ironclad, Character::Silent => Self::Silent,
            Character::Defect => Self::Defect, Character::Regent => Self::Regent,
            Character::Necrobinder => Self::Necrobinder,
        }
    }
}
