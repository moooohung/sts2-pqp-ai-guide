//! Public option consequences from Models/Events/*.cs in capture 0861BFA1.
//! No option is evaluated by executing its callback, selecting a reward or reading RNG.
use crate::{card::Card, identity::{EnchantmentId, EventOptionId}, tables::*};
use super::{EventDescription as D, EventOptionIdentity, EventParameter, EventDish};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicEventTiming { Immediate, OnSelection, AfterVictory, FutureCombat, Confirmation }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicEventSubject {
    Card(CardId), Relic(RelicId), Potion(PotionId), Enchantment(EnchantmentId), CardType(CardType),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PublicEventParameter {
    pub variable: &'static str,
    pub amount: i32,
    pub fraction: [f32; 2],
    pub timing: PublicEventTiming,
    pub subject: Option<PublicEventSubject>,
}
impl PublicEventParameter {
    pub const fn integer(variable: &'static str, amount: i32) -> Self {
        Self { variable, amount, fraction: [0.; 2], timing: PublicEventTiming::Immediate, subject: None }
    }
    pub fn decimal(variable: &'static str, value: f64) -> Result<Self, &'static str> {
        let whole = value.trunc();
        let fraction = (value - whole) as f32;
        let tail = (value - whole - f64::from(fraction)) as f32;
        if !value.is_finite() || whole < i32::MIN as f64 || whole > i32::MAX as f64
            || whole + f64::from(fraction) + f64::from(tail) != value {
            return Err("public event amount needs exact numeric representation");
        }
        Ok(Self { amount: whole as i32, fraction: [fraction, tail], ..Self::integer(variable, 0) })
    }
    pub const fn at(mut self, timing: PublicEventTiming) -> Self { self.timing = timing; self }
    pub const fn subject(mut self, subject: PublicEventSubject) -> Self { self.subject = Some(subject); self }
}

/// Only values already displayed by the current event, never the event's raw scratch/RNG.
#[derive(Clone, Copy, Debug)]
pub enum PublicEventDynamic { None, Price(i32), Damage(i32), Stage(i32), CardType(CardType) }
#[derive(Clone, Copy)]
pub struct PublicEventContext<'a> {
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub deck: &'a [Card],
    pub players: usize,
    pub description: Option<D>,
    pub dynamic: PublicEventDynamic,
}

pub(crate) fn event_dynamic(event: Option<&super::events::EventState>) -> Result<PublicEventDynamic, &'static str> {
    use crate::identity::EventId as E;
    let Some(event) = event else { return Ok(PublicEventDynamic::None); };
    Ok(match event.id {
        E::AbyssalBaths => PublicEventDynamic::Damage(event.ints[0]),
        E::CrystalSphere | E::LuminousChoir | E::WhisperingHollow => PublicEventDynamic::Price(event.ints[0]),
        E::TabletOfTruth => PublicEventDynamic::Stage(event.ints[0]),
        E::WarHistorianRepy => PublicEventDynamic::Stage(i32::from(event.options.page.id != event.id.initial_page())),
        E::TinkerTime => PublicEventDynamic::CardType(match event.ints[1] {
            0 => CardType::Attack, 1 => CardType::Skill, 2 => CardType::Power,
            _ => return Err("invalid displayed TinkerTime type"),
        }),
        _ => PublicEventDynamic::None,
    })
}

/// Exhaustive over the original option registry, including locked/continuation rows.
/// Quantities are the displayed offer, not a prediction of a hidden random selection.
pub fn public_event_parameters(option: EventOptionIdentity, ctx: PublicEventContext<'_>)
    -> Result<Vec<PublicEventParameter>, &'static str> {
    use EventOptionId::*;
    use PublicEventTiming::*;
    use PublicEventSubject as S;
    let mut out = Vec::new();
    macro_rules! p {
        ($name:literal, $n:expr) => {{ out.push(PublicEventParameter::integer($name, $n)); }};
        ($name:literal, $n:expr, $timing:expr) => {{ out.push(PublicEventParameter::integer($name, $n).at($timing)); }};
    }
    macro_rules! subject {
        ($name:literal, $n:expr, $subject:expr) => {{
            out.push(PublicEventParameter::integer($name, $n).subject($subject));
        }};
    }
    macro_rules! enchant {
        ($n:expr, $amount:expr, $id:ident) => {
            subject!("CardsEnchant", $n, S::Enchantment(EnchantmentId::$id));
            subject!("EnchantmentAmount", $amount, S::Enchantment(EnchantmentId::$id));
        };
    }
    let price = || match ctx.dynamic { PublicEventDynamic::Price(n) => Ok(n), _ => Err("displayed event price missing") };
    let displayed = || -> Result<i32, &'static str> {
        Ok(match ctx.description {
            Some(D::DenseVegetationTrudgeGold(n) | D::DenseVegetationRestHeal(n)
                | D::ColossalFlowerExtractGold(n) | D::ColossalFlowerReachDeeperDamage(n)
                | D::ColossalFlowerPollinousCoreDamage(n) | D::SunkenTreasurySmallGold(n)
                | D::SunkenTreasuryLargeGold(n) | D::SunkenStatueDive(n)
                | D::LostWispSearchGold(n) | D::UnrestSiteHeal(n) | D::SpiralingWhirlpoolHeal(n)
                | D::ThisOrThatPlainGold(n) | D::TabletDecipherCost(n)
                | D::BridgeHoldOn(n) | D::FakeMerchantCost(n)) => n,
            _ => return Err("displayed dynamic event amount missing"),
        })
    };
    match option.id {
        AbyssalBathsInitialImmerse | AbyssalBathsAllLinger => {
            let PublicEventDynamic::Damage(n) = ctx.dynamic else { return Err("displayed bath damage missing"); };
            p!("HpCost", n); p!("MaxHpGain", 2);
        }
        AbyssalBathsInitialAbstain => p!("Heal", 10),
        AbyssalBathsAllExitBaths | EndlessConveyorGrabSomethingOffTheBeltLeave |
        RelicTraderProceed | TabletOfTruthDecipherGiveUp | SelfHelpBookInitialNoOptions |
        FakeMerchantNFakeMerchantHideScreen | MockEventModelInitialTest |
        TheArchitectDialogueCurrentLineIndex => p!("Continue", 1),
        AmalgamatorInitialCombineStrikes => { p!("CardsRemove", 2); subject!("CardsGain", 1, S::Card(CardId::UltimateStrike)); }
        AmalgamatorInitialCombineDefends => { p!("CardsRemove", 2); subject!("CardsGain", 1, S::Card(CardId::UltimateDefend)); }
        AromaOfChaosInitialLetGo | EndlessConveyorInitialObserveChef => {
            if option.id == AromaOfChaosInitialLetGo { p!("CardsTransform", 1); } else { p!("CardsUpgrade", 1); }
        }
        AromaOfChaosInitialMaintainControl | DoorsOfLightAndDarkInitialLight => {
            p!("CardsUpgrade", if option.id == DoorsOfLightAndDarkInitialLight { 2 } else { 1 });
        }
        BattlewornDummyInitialSetting1 | BattlewornDummyInitialSetting2 | BattlewornDummyInitialSetting3 => {
            p!("Combat", 1); p!("VictoryTurnLimit", 3);
            match option.id {
                BattlewornDummyInitialSetting1 => p!("PotionsGain", 1, AfterVictory),
                BattlewornDummyInitialSetting2 => p!("CardsUpgrade", 2, AfterVictory),
                _ => p!("RelicsGain", 1, AfterVictory),
            }
        }
        BrainLeechInitialRip => { p!("HpCost", 5); p!("CardsGain", 1, OnSelection); p!("CardChoices", 3); }
        BrainLeechInitialShareKnowledge => { p!("CardsGain", 1, OnSelection); p!("CardChoices", 5); }
        BugslayerInitialExtermination => subject!("CardsGain", 1, S::Card(CardId::Exterminate)),
        BugslayerInitialSquash => subject!("CardsGain", 1, S::Card(CardId::Squash)),
        ByrdonisNestInitialEat => p!("MaxHpGain", 7),
        ByrdonisNestInitialTake => subject!("CardsGain", 1, S::Card(CardId::ByrdonisEgg)),
        ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant => { p!("CardsGain", 3, OnSelection); p!("CardChoices", 3); }
        ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1 |
        ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1 |
        ColossalFlowerReachDeeper2ExtractInstead => p!("GoldGain", displayed()?),
        ColossalFlowerInitialReachDeeperNumberOfDigs1 |
        ColossalFlowerReachDeeperNumberOfDigsReachDeeperNumberOfDigs1 => {
            p!("HpCost", displayed()?); p!("Continue", 1);
        }
        ColossalFlowerReachDeeper2PollinousCore => {
            p!("HpCost", displayed()?); subject!("RelicsGain", 1, S::Relic(RelicId::PollinousCore));
        }
        CrystalSphereInitialUncoverFuture => { p!("GoldCost", price()?); p!("DivinationsGain", 3); }
        CrystalSphereInitialPaymentPlan => { subject!("CursesGain", 1, S::Card(CardId::Debt)); p!("DivinationsGain", 6); }
        CrystalSphereCrystalSphereMinigameCellClicked => p!("DivinationsCost", 1),
        DarvPageNameRelicIdEntry | HungryForMushroomsPageNameRelicIdEntry |
        NeowPageNameRelicIdEntry | NonupeipePageNameRelicIdEntry | OrobasPageNameRelicIdEntry |
        PaelPageNameRelicIdEntry | TanxPageNameRelicIdEntry | TezcataraPageNameRelicIdEntry |
        VakuuPageNameRelicIdEntry | DollRoomTakeTake => p!("RelicsGain", 1),
        OrobasInitialOptionPool3Locked | GraveOfTheForgottenInitialConfrontLocked |
        SelfHelpBookInitialReadTheBackLocked | SelfHelpBookInitialReadPassageLocked |
        SelfHelpBookInitialReadEntireBookLocked | StoneOfAllTimeInitialLiftLocked |
        StoneOfAllTimeInitialPushLocked | SymbioteInitialApproachLocked |
        WoodCarvingsInitialSnakeLocked | ZenWeaverInitialLocked |
        RanwidTheElderInitialPotionLocked | RanwidTheElderInitialRelicLocked => p!("Locked", 1),
        DenseVegetationInitialTrudgeOn => { p!("HpCost", 8); p!("GoldGain", displayed()?); }
        DenseVegetationInitialRest => { p!("Heal", displayed()?); p!("Combat", 1, OnSelection); }
        DenseVegetationRestFight => p!("Combat", 1),
        DollRoomInitialRandom => p!("RelicsGain", 1),
        DollRoomInitialTakeSomeTime => { p!("HpCost", 5); p!("RelicsGain", 1, OnSelection); p!("RelicChoices", 2); }
        DollRoomInitialExamine => { p!("HpCost", 15); p!("RelicsGain", 1, OnSelection); p!("RelicChoices", 3); }
        DoorsOfLightAndDarkInitialDark => p!("CardsRemove", 1),
        DrowningBeaconInitialBottle => subject!("PotionsGain", 1, S::Potion(PotionId::Glowwaterpotion)),
        DrowningBeaconInitialClimb => { p!("MaxHpCost", 13); subject!("RelicsGain", 1, S::Relic(RelicId::FresnelLens)); }
        EndlessConveyorAllLocked => p!("GoldCost", 40),
        EndlessConveyorAllId => {
            let EventParameter::ConveyorDish(dish) = option.parameter else { return Err("displayed conveyor dish missing"); };
            if dish != EventDish::Fysh { p!("GoldCost", 40); }
            match dish {
                EventDish::Caviar => p!("MaxHpGain", 4),
                EventDish::Spicy => p!("CardsUpgrade", 1),
                EventDish::Jelly => p!("CardsTransform", 1),
                EventDish::Eel => p!("CardsGain", 1),
                EventDish::Condiment => p!("PotionsGain", 1),
                EventDish::Clam => p!("Heal", 10),
                EventDish::Fysh => p!("GoldGain", 75),
                EventDish::Salad => subject!("CardsGain", 1, S::Card(CardId::FeedingFrenzyCard)),
            }
        }
        FakeMerchantNFakeMerchantMerchantRelicEntry => { p!("GoldCost", displayed()?); p!("RelicsGain", 1); }
        FieldOfManSizedHolesInitialResist => { p!("CardsRemove", 2); subject!("CursesGain", 1, S::Card(CardId::Normality)); }
        FieldOfManSizedHolesInitialEnterYourHole => { enchant!(1, 1, PerfectFit); }
        GraveOfTheForgottenInitialConfront => { subject!("CursesGain", 1, S::Card(CardId::Decay)); enchant!(1, 1, SoulsPower); }
        GraveOfTheForgottenInitialAccept => subject!("RelicsGain", 1, S::Relic(RelicId::ForgottenSoul)),
        InfestedAutomatonInitialStudy => subject!("CardsGain", 1, S::CardType(CardType::Power)),
        InfestedAutomatonInitialTouchCore => { p!("CardsGain", 1); p!("CardEnergyCost", 0); }
        JungleMazeAdventureInitialSoloQuest | JungleMazeAdventureInitialJoinForces => {
            let Some(D::JungleSoloGold(bits) | D::JungleJoinGold(bits)) = ctx.description else { return Err("displayed jungle gold missing"); };
            out.push(PublicEventParameter::decimal("GoldGain", f64::from_bits(bits))?);
            if option.id == JungleMazeAdventureInitialSoloQuest { p!("HpCost", 18); }
        }
        LostWispInitialClaim => { subject!("CursesGain", 1, S::Card(CardId::Decay)); subject!("RelicsGain", 1, S::Relic(RelicId::LostWisp)); }
        LostWispInitialSearch => p!("GoldGain", displayed()?),
        LuminousChoirInitialReachIntoTheFlesh => { p!("CardsRemove", 2); subject!("CursesGain", 1, S::Card(CardId::SporeMind)); }
        LuminousChoirInitialOfferTribute => { p!("GoldCost", price()?); p!("RelicsGain", 1); }
        LuminousChoirInitialOfferTributeLocked => p!("GoldCost", price()?),
        MorphicGroveInitialLoner => p!("MaxHpGain", 5),
        MorphicGroveInitialGroup => { p!("GoldCost", ctx.gold); p!("CardsTransform", 2); }
        PotionCourierInitialGrabPotions => subject!("PotionsGain", 3, S::Potion(PotionId::Foulpotion)),
        PotionCourierInitialRansack => p!("PotionsGain", 1),
        PunchOffInitialNab => { subject!("CursesGain", 1, S::Card(CardId::Injury)); p!("RelicsGain", 1); }
        PunchOffInitialICanTakeThem | PunchOffICanTakeThemFight => {
            p!("Combat", 1); p!("RelicsGain", 1, AfterVictory); p!("PotionsGain", 1, AfterVictory);
        }
        RanwidTheElderInitialGold => { p!("GoldCost", 100); p!("RelicsGain", 1); }
        RanwidTheElderInitialPotion => { p!("PotionsCost", 1); p!("RelicsGain", 1); }
        RanwidTheElderInitialRelic => { p!("RelicsCost", 1); p!("RelicsGain", 2); }
        ReflectionsInitialTouchAMirror => { p!("CardsDowngrade", 2); p!("CardsUpgrade", 4); }
        ReflectionsInitialShatter => { p!("CardsDuplicate", ctx.deck.len() as i32); subject!("CursesGain", 1, S::Card(CardId::BadLuck)); }
        RelicTraderInitialTop | RelicTraderInitialMiddle | RelicTraderInitialBottom => { p!("RelicsCost", 1); p!("RelicsGain", 1); }
        RoomFullOfCheeseInitialGorge => { p!("CardsGain", 2, OnSelection); p!("CardChoices", 8); }
        RoomFullOfCheeseInitialSearch => { p!("HpCost", 14); subject!("RelicsGain", 1, S::Relic(RelicId::ChosenCheese)); }
        RoundTeaPartyInitialEnjoyTea => { p!("Heal", ctx.max_hp - ctx.hp); subject!("RelicsGain", 1, S::Relic(RelicId::RoyalPoison)); }
        RoundTeaPartyInitialPickFight => { p!("HpCost", 11, OnSelection); p!("RelicsGain", 1, OnSelection); }
        RoundTeaPartyPickFightContinueFight => { p!("HpCost", 11); p!("RelicsGain", 1); }
        SapphireSeedInitialEat => { p!("Heal", 9); p!("CardsUpgrade", 1); }
        SapphireSeedInitialPlant => { enchant!(1, 1, Sown); }
        SelfHelpBookInitialReadTheBack => { enchant!(1, 2, Sharp); }
        SelfHelpBookInitialReadPassage => { enchant!(1, 2, Nimble); }
        SelfHelpBookInitialReadEntireBook => { enchant!(1, 2, Swift); }
        SlipperyBridgeInitialOvercome => p!("CardsRemove", 1),
        SlipperyBridgeInitialHoldOn0 | SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2 => {
            p!("HpCost", displayed()?); p!("Continue", 1);
        }
        SpiralingWhirlpoolInitialDrink => p!("Heal", displayed()?),
        SpiralingWhirlpoolInitialObserve => { enchant!(1, 1, Spiral); }
        SpiritGrafterInitialLetItIn => { p!("Heal", 25); subject!("CardsGain", 1, S::Card(CardId::Metamorphosis)); }
        SpiritGrafterInitialRejection => { p!("HpCost", 10); p!("CardsUpgrade", 1); }
        StoneOfAllTimeInitialLift => { p!("PotionsCost", 1); p!("MaxHpGain", 10); }
        StoneOfAllTimeInitialPush => { p!("HpCost", 6); enchant!(1, 8, Vigorous); }
        SunkenStatueInitialGrabSword => subject!("RelicsGain", 1, S::Relic(RelicId::SwordOfStone)),
        SunkenStatueInitialDiveIntoWater => { p!("HpCost", 7); p!("GoldGain", displayed()?); }
        SunkenTreasuryInitialFirstChest => p!("GoldGain", displayed()?),
        SunkenTreasuryInitialSecondChest => { p!("GoldGain", displayed()?); subject!("CursesGain", 1, S::Card(CardId::Greed)); }
        SymbioteInitialApproach => { enchant!(1, 1, Corrupted); }
        SymbioteInitialKillWithFire => p!("CardsTransform", 1),
        TabletOfTruthInitialSmash => p!("Heal", 20),
        TabletOfTruthInitialDecipher1 | TabletOfTruthDecipherDecipherCountDecipher => {
            p!("MaxHpCost", displayed()?);
            let PublicEventDynamic::Stage(stage) = ctx.dynamic else { return Err("displayed decipher stage missing"); };
            p!("CardsUpgrade", if stage == 4 { ctx.deck.iter().filter(|c| !c.upgraded
                && card_info(c.id, false).is_some_and(|i| i.upgradable)).count() as i32 } else { 1 });
        }
        TeaMasterInitialBoneTea | TeaMasterInitialBoneTeaLocked => {
            p!("GoldCost", 50);
            if option.id == TeaMasterInitialBoneTea { subject!("RelicsGain", 1, S::Relic(RelicId::BoneTea)); }
        }
        TeaMasterInitialEmberTea | TeaMasterInitialEmberTeaLocked => {
            p!("GoldCost", 150);
            if option.id == TeaMasterInitialEmberTea { subject!("RelicsGain", 1, S::Relic(RelicId::EmberTea)); }
        }
        TeaMasterInitialTeaOfDiscourtesy => subject!("RelicsGain", 1, S::Relic(RelicId::TeaOfDiscourtesy)),
        TheArchitectProceed => p!("WinRun", 1),
        TheFutureOfPotionsInitialPotion => { p!("PotionsCost", 1); p!("CardsGain", 1, OnSelection); p!("CardChoices", 3); p!("CardsUpgrade", 1, OnSelection); }
        TheLanternKeyInitialReturnTheKey | TheLanternKeyDoneReturnTheKey => p!("GoldGain", 100),
        TheLanternKeyInitialKeepTheKey | TheLanternKeyKeepTheKeyFight => {
            p!("Combat", 1); out.push(PublicEventParameter::integer("CardsGain", 1).subject(S::Card(CardId::LanternKey)).at(AfterVictory));
        }
        TheLegendsWereTrueInitialNabTheMap => subject!("CardsGain", 1, S::Card(CardId::SpoilsMap)),
        TheLegendsWereTrueInitialSlowlyFindAnExit => { p!("HpCost", 8); p!("PotionsGain", 1); }
        ThisOrThatInitialPlain => { p!("HpCost", 6); p!("GoldGain", displayed()?); }
        ThisOrThatInitialOrnate => { p!("RelicsGain", 1); subject!("CursesGain", 1, S::Card(CardId::Clumsy)); }
        TinkerTimeInitialChooseCardType => p!("Continue", 1),
        TinkerTimeChooseCardTypeAttack => { subject!("CardType", 1, S::CardType(CardType::Attack)); p!("Damage", 12, OnSelection); }
        TinkerTimeChooseCardTypeSkill => { subject!("CardType", 1, S::CardType(CardType::Skill)); p!("Block", 8, OnSelection); }
        TinkerTimeChooseCardTypePower => { subject!("CardType", 1, S::CardType(CardType::Power)); p!("Continue", 1); }
        TinkerTimeChooseRiderChaos | TinkerTimeChooseRiderChoking | TinkerTimeChooseRiderCurious |
        TinkerTimeChooseRiderEnergized | TinkerTimeChooseRiderExpertise | TinkerTimeChooseRiderImprovement |
        TinkerTimeChooseRiderSapping | TinkerTimeChooseRiderViolence | TinkerTimeChooseRiderWisdom => {
            subject!("CardsGain", 1, S::Card(CardId::MadScience)); p!("CardEnergyCost", 1);
            let PublicEventDynamic::CardType(kind) = ctx.dynamic else { return Err("displayed TinkerTime card type missing"); };
            match kind { CardType::Attack => p!("Damage", 12), CardType::Skill => p!("Block", 8), _ => {} }
            match option.id {
                TinkerTimeChooseRiderSapping => { p!("WeakPower", 2); p!("VulnerablePower", 2); }
                TinkerTimeChooseRiderViolence => p!("Repeat", 3),
                TinkerTimeChooseRiderChoking => p!("StranglePower", 6),
                TinkerTimeChooseRiderEnergized => p!("Energy", 2),
                TinkerTimeChooseRiderWisdom => p!("Cards", 3),
                TinkerTimeChooseRiderExpertise => { p!("StrengthPower", 2); p!("DexterityPower", 2); }
                TinkerTimeChooseRiderCurious => p!("CuriousReduction", 1),
                TinkerTimeChooseRiderChaos => { p!("CardsGenerate", 1, FutureCombat); p!("GeneratedCardEnergyCost", 0, FutureCombat); }
                TinkerTimeChooseRiderImprovement => p!("ImprovementPower", 1, FutureCombat),
                _ => unreachable!("rider match is exhaustive above"),
            }
        }
        TrashHeapInitialDiveIn => { p!("HpCost", 8); p!("RelicsGain", 1); }
        TrashHeapInitialGrab => { p!("GoldGain", 100); p!("CardsGain", 1); }
        TrialInitialAccept | TrialInitialReject | TrialRejectAccept => p!("Continue", 1),
        TrialRejectDoubleDown => p!("AbandonRun", 1, Confirmation),
        TrialMerchantGuilty => { subject!("CursesGain", 1, S::Card(CardId::Regret)); p!("RelicsGain", 2); }
        TrialMerchantInnocent => { subject!("CursesGain", 1, S::Card(CardId::Shame)); p!("CardsUpgrade", 2); }
        TrialNobleGuilty => p!("Heal", 10),
        TrialNobleInnocent => { subject!("CursesGain", 1, S::Card(CardId::Regret)); p!("GoldGain", 300); }
        TrialNondescriptGuilty => { subject!("CursesGain", 1, S::Card(CardId::Doubt)); p!("CardsGain", 2, OnSelection); p!("CardChoices", 3); }
        TrialNondescriptInnocent => { subject!("CursesGain", 1, S::Card(CardId::Doubt)); p!("CardsTransform", 2); }
        UnrestSiteInitialRest => { p!("Heal", displayed()?); subject!("CursesGain", 1, S::Card(CardId::PoorSleep)); }
        UnrestSiteInitialKill => { p!("MaxHpCost", 8); p!("RelicsGain", 1); }
        WarHistorianRepyInitialUnlockCage | WarHistorianRepyInitialUnlockChest => {
            let keys = ctx.deck.iter().filter(|c| c.id == CardId::LanternKey).count() as i32;
            let n = if ctx.players == 1 && matches!(ctx.dynamic, PublicEventDynamic::Stage(0)) { keys.min(1) } else { keys };
            subject!("CardsRemove", n, S::Card(CardId::LanternKey));
            if option.id == WarHistorianRepyInitialUnlockCage { subject!("RelicsGain", 1, S::Relic(RelicId::HistoryCourse)); }
            else { p!("RelicsGain", 2); p!("PotionsGain", 2); }
        }
        WaterloggedScriptoriumInitialBloodyInk => p!("MaxHpGain", 6),
        WaterloggedScriptoriumInitialTentacleQuill | WaterloggedScriptoriumInitialTentacleQuillLocked => {
            p!("GoldCost", 55);
            if option.id == WaterloggedScriptoriumInitialTentacleQuill { enchant!(1, 1, Steady); }
        }
        WaterloggedScriptoriumInitialPricklySponge | WaterloggedScriptoriumInitialPricklySpongeLocked => {
            p!("GoldCost", 99);
            if option.id == WaterloggedScriptoriumInitialPricklySponge { enchant!(2, 1, Steady); }
        }
        WelcomeToWongosInitialBargainBin | WelcomeToWongosInitialBargainBinLocked => {
            p!("GoldCost", 100); if option.id == WelcomeToWongosInitialBargainBin { p!("RelicsGain", 1); }
        }
        WelcomeToWongosInitialFeaturedItem | WelcomeToWongosInitialFeaturedItemLocked => {
            p!("GoldCost", 200); if option.id == WelcomeToWongosInitialFeaturedItem { p!("RelicsGain", 1); }
        }
        WelcomeToWongosInitialMysteryBox | WelcomeToWongosInitialMysteryBoxLocked => {
            p!("GoldCost", 300);
            if option.id == WelcomeToWongosInitialMysteryBox { p!("RelicsGain", 3, FutureCombat); p!("Combats", 5); }
        }
        WelcomeToWongosInitialLeave => p!("CardsDowngrade", 1),
        WellspringInitialBottle => p!("PotionsGain", 1),
        WellspringInitialBathe => { p!("CardsRemove", 1); subject!("CursesGain", 1, S::Card(CardId::Guilty)); }
        WhisperingHollowInitialGold => { p!("GoldCost", price()?); p!("PotionsGain", 2); }
        WhisperingHollowInitialHug => { p!("HpCost", 9); p!("CardsTransform", 1); }
        WoodCarvingsInitialBird => { p!("CardsTransform", 1); subject!("CardsGain", 1, S::Card(CardId::Peck)); }
        WoodCarvingsInitialSnake => { enchant!(1, 1, Slither); }
        WoodCarvingsInitialTorus => { p!("CardsTransform", 1); subject!("CardsGain", 1, S::Card(CardId::ToricToughness)); }
        ZenWeaverInitialBreathingTechniques => { p!("GoldCost", 50); subject!("CardsGain", 2, S::Card(CardId::Enlightenment)); }
        ZenWeaverInitialEmotionalAwareness => { p!("GoldCost", 125); p!("CardsRemove", 1); }
        ZenWeaverInitialArachnidAcupuncture => { p!("GoldCost", 250); p!("CardsRemove", 2); }
    }
    Ok(out)
}
