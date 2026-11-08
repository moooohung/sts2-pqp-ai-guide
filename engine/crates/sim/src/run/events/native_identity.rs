//! Retained event identities. Compatibility menu templates are converted once
//! when an offer is constructed; native observation reads only typed fields.
use super::{EventDescription, EventOption};
use crate::identity::{EventId, EventOptionId, EventPageId};
use crate::tables::RelicId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventParameter {
    None,
    Offer(u16),
    Counter(i32),
    /// Original NAbandonRunConfirmPopup, not an EventOption registry entry.
    AbandonRunConfirmation(bool),
    /// Legacy adapter for the original post-minigame reward screen.
    SphereReward { offer:u16, action:u8 },
    /// SlipperyBridge.GetHoldOnSuffix: None is the original LOOP suffix.
    Bridge(Option<u8>),
    ConveyorDish(crate::run::manager::conveyor::Dish),
    Relic(RelicId),
    Sphere { big: bool, x: u8, y: u8 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventPageIdentity { pub id: EventPageId, pub parameter: EventParameter }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventOptionIdentity { pub id: EventOptionId, pub parameter: EventParameter }

pub(crate) trait EventOptionSelector {
    fn resolve(self, event: &super::EventState) -> EventOptionIdentity;
}

impl EventOptionSelector for EventOptionIdentity {
    #[inline]
    fn resolve(self, _event: &super::EventState) -> EventOptionIdentity { self }
}

#[cfg(test)]
impl EventOptionSelector for &str {
    #[inline]
    fn resolve(self, event: &super::EventState) -> EventOptionIdentity {
        if let Some(option) = event.options.iter().find(|option| option.option_id.legacy_id() == self) {
            return option.option_id;
        }
        EventOptionIdentity::from_legacy(event.id, event.options.page, self)
    }
}

#[cfg(test)]
impl PartialEq<&str> for EventOptionIdentity {
    fn eq(&self, other: &&str) -> bool { self.legacy_id() == *other }
}

// Darv.GenerateInitialOptions takes three Ancient offers. CallingBell.AfterObtained
// then offers exactly three RelicRewards (Common, Uncommon, Rare) through
// RewardsCmd.OfferCustom. The multiplayer event adapter retains those pickup
// choices and its proceed action in this parent event's menu. Keep their
// parameters disjoint from the initial offers, without inventing registry IDs.
const DARV_MENU_KEYS: [&str; 7] = [
    "relic_1", "relic_2", "relic_3", "bell_0", "bell_1", "bell_2", "skip_bell",
];

#[cfg(test)]
mod pickup_tests;

macro_rules! fixed_options {
    ($(($event:ident, $key:literal, $id:ident)),* $(,)?) => {
        fn fixed_from_legacy(event: EventId, key: &str) -> Option<EventOptionId> {
            match (event, key) {
                $((EventId::$event, $key) => Some(EventOptionId::$id),)*
                _ => None,
            }
        }
        fn fixed_to_legacy(id: EventOptionId) -> Option<&'static str> {
            match id { $(EventOptionId::$id => Some($key),)* _ => None }
        }
    };
}
// Original identities are explicit. Adapter spellings are never learned IDs.
fixed_options! {
    (AbyssalBaths, "immerse", AbyssalBathsInitialImmerse),
    (AbyssalBaths, "abstain", AbyssalBathsInitialAbstain),
    (AbyssalBaths, "linger", AbyssalBathsAllLinger),
    (AbyssalBaths, "exit_baths", AbyssalBathsAllExitBaths),
    (Amalgamator, "combine_strikes", AmalgamatorInitialCombineStrikes),
    (Amalgamator, "combine_defends", AmalgamatorInitialCombineDefends),
    (AromaOfChaos, "let_go", AromaOfChaosInitialLetGo),
    (AromaOfChaos, "maintain_control", AromaOfChaosInitialMaintainControl),
    (BrainLeech, "share_knowledge", BrainLeechInitialShareKnowledge),
    (BrainLeech, "rip", BrainLeechInitialRip),
    (Bugslayer, "exterminate", BugslayerInitialExtermination),
    (Bugslayer, "squash", BugslayerInitialSquash),
    (ByrdonisNest, "eat", ByrdonisNestInitialEat),
    (ByrdonisNest, "take", ByrdonisNestInitialTake),
    (CrystalSphere, "pay", CrystalSphereInitialUncoverFuture),
    (CrystalSphere, "debt", CrystalSphereInitialPaymentPlan),
    (DenseVegetation, "trudge_on", DenseVegetationInitialTrudgeOn),
    (DenseVegetation, "rest", DenseVegetationInitialRest),
    (DenseVegetation, "fight", DenseVegetationRestFight),
    (DollRoom, "random", DollRoomInitialRandom),
    (DollRoom, "take_time", DollRoomInitialTakeSomeTime),
    (DollRoom, "examine", DollRoomInitialExamine),
    (DoorsOfLightAndDark, "light", DoorsOfLightAndDarkInitialLight),
    (DoorsOfLightAndDark, "dark", DoorsOfLightAndDarkInitialDark),
    (DrowningBeacon, "bottle", DrowningBeaconInitialBottle),
    (DrowningBeacon, "climb", DrowningBeaconInitialClimb),
    (EndlessConveyor, "grab", EndlessConveyorAllId),
    (EndlessConveyor, "observe", EndlessConveyorInitialObserveChef),
    (EndlessConveyor, "leave", EndlessConveyorGrabSomethingOffTheBeltLeave),
    (FakeMerchant, "leave", FakeMerchantNFakeMerchantHideScreen),
    (FieldOfManSizedHoles, "resist", FieldOfManSizedHolesInitialResist),
    (FieldOfManSizedHoles, "enter", FieldOfManSizedHolesInitialEnterYourHole),
    (GraveOfTheForgotten, "confront", GraveOfTheForgottenInitialConfront),
    (GraveOfTheForgotten, "accept", GraveOfTheForgottenInitialAccept),
    (InfestedAutomaton, "study", InfestedAutomatonInitialStudy),
    (InfestedAutomaton, "touch_core", InfestedAutomatonInitialTouchCore),
    (JungleMazeAdventure, "solo", JungleMazeAdventureInitialSoloQuest),
    (JungleMazeAdventure, "join", JungleMazeAdventureInitialJoinForces),
    (LostWisp, "claim", LostWispInitialClaim),
    (LostWisp, "search", LostWispInitialSearch),
    (LuminousChoir, "reach", LuminousChoirInitialReachIntoTheFlesh),
    (LuminousChoir, "tribute", LuminousChoirInitialOfferTribute),
    (MorphicGrove, "group", MorphicGroveInitialGroup),
    (MorphicGrove, "loner", MorphicGroveInitialLoner),
    (PotionCourier, "grab", PotionCourierInitialGrabPotions),
    (PotionCourier, "ransack", PotionCourierInitialRansack),
    (PunchOff, "nab", PunchOffInitialNab),
    (PunchOff, "take_them", PunchOffInitialICanTakeThem),
    (PunchOff, "fight", PunchOffICanTakeThemFight),
    (RanwidTheElder, "potion", RanwidTheElderInitialPotion),
    (RanwidTheElder, "gold", RanwidTheElderInitialGold),
    (RanwidTheElder, "relic", RanwidTheElderInitialRelic),
    (RelicTrader, "trade_0", RelicTraderInitialTop),
    (RelicTrader, "trade_1", RelicTraderInitialMiddle),
    (RelicTrader, "trade_2", RelicTraderInitialBottom),
    (RelicTrader, "proceed", RelicTraderProceed),
    (RoomFullOfCheese, "gorge", RoomFullOfCheeseInitialGorge),
    (RoomFullOfCheese, "search", RoomFullOfCheeseInitialSearch),
    (RoundTeaParty, "enjoy_tea", RoundTeaPartyInitialEnjoyTea),
    (RoundTeaParty, "pick_fight", RoundTeaPartyInitialPickFight),
    (RoundTeaParty, "continue_fight", RoundTeaPartyPickFightContinueFight),
    (SapphireSeed, "eat", SapphireSeedInitialEat),
    (SapphireSeed, "plant", SapphireSeedInitialPlant),
    (SelfHelpBook, "read_back", SelfHelpBookInitialReadTheBack),
    (SelfHelpBook, "read_passage", SelfHelpBookInitialReadPassage),
    (SelfHelpBook, "read_entire", SelfHelpBookInitialReadEntireBook),
    (SelfHelpBook, "skip_book", SelfHelpBookInitialNoOptions),
    (SlipperyBridge, "overcome", SlipperyBridgeInitialOvercome),
    (SpiralingWhirlpool, "observe", SpiralingWhirlpoolInitialObserve),
    (SpiralingWhirlpool, "drink", SpiralingWhirlpoolInitialDrink),
    (SpiritGrafter, "let_it_in", SpiritGrafterInitialLetItIn),
    (SpiritGrafter, "rejection", SpiritGrafterInitialRejection),
    (StoneOfAllTime, "lift", StoneOfAllTimeInitialLift),
    (StoneOfAllTime, "push", StoneOfAllTimeInitialPush),
    (SunkenStatue, "grab_sword", SunkenStatueInitialGrabSword),
    (SunkenStatue, "dive", SunkenStatueInitialDiveIntoWater),
    (SunkenTreasury, "first_chest", SunkenTreasuryInitialFirstChest),
    (SunkenTreasury, "second_chest", SunkenTreasuryInitialSecondChest),
    (Symbiote, "approach", SymbioteInitialApproach),
    (Symbiote, "kill_fire", SymbioteInitialKillWithFire),
    (TabletOfTruth, "smash", TabletOfTruthInitialSmash),
    (TabletOfTruth, "give_up", TabletOfTruthDecipherGiveUp),
    (TeaMaster, "bone_tea", TeaMasterInitialBoneTea),
    (TeaMaster, "ember_tea", TeaMasterInitialEmberTea),
    (TeaMaster, "discourtesy", TeaMasterInitialTeaOfDiscourtesy),
    (TheArchitect, "advance", TheArchitectDialogueCurrentLineIndex),
    (TheArchitect, "proceed", TheArchitectProceed),
    (TheLanternKey, "return_key", TheLanternKeyInitialReturnTheKey),
    (TheLanternKey, "keep_key", TheLanternKeyInitialKeepTheKey),
    (TheLanternKey, "fight", TheLanternKeyKeepTheKeyFight),
    (TheLegendsWereTrue, "nab_map", TheLegendsWereTrueInitialNabTheMap),
    (TheLegendsWereTrue, "find_exit", TheLegendsWereTrueInitialSlowlyFindAnExit),
    (ThisOrThat, "plain", ThisOrThatInitialPlain),
    (ThisOrThat, "ornate", ThisOrThatInitialOrnate),
    (TrashHeap, "dive_in", TrashHeapInitialDiveIn),
    (TrashHeap, "grab", TrashHeapInitialGrab),
    (UnrestSite, "rest", UnrestSiteInitialRest),
    (UnrestSite, "kill", UnrestSiteInitialKill),
    (WaterloggedScriptorium, "bloody_ink", WaterloggedScriptoriumInitialBloodyInk),
    (WaterloggedScriptorium, "tentacle_quill", WaterloggedScriptoriumInitialTentacleQuill),
    (WaterloggedScriptorium, "prickly_sponge", WaterloggedScriptoriumInitialPricklySponge),
    (WelcomeToWongos, "bargain_bin", WelcomeToWongosInitialBargainBin),
    (WelcomeToWongos, "featured", WelcomeToWongosInitialFeaturedItem),
    (WelcomeToWongos, "mystery", WelcomeToWongosInitialMysteryBox),
    (WelcomeToWongos, "leave", WelcomeToWongosInitialLeave),
    (Wellspring, "bottle", WellspringInitialBottle),
    (Wellspring, "bathe", WellspringInitialBathe),
    (WhisperingHollow, "gold", WhisperingHollowInitialGold),
    (WhisperingHollow, "hug", WhisperingHollowInitialHug),
    (WoodCarvings, "bird", WoodCarvingsInitialBird),
    (WoodCarvings, "snake", WoodCarvingsInitialSnake),
    (WoodCarvings, "torus", WoodCarvingsInitialTorus),
    (ZenWeaver, "breathing", ZenWeaverInitialBreathingTechniques),
    (ZenWeaver, "emotional", ZenWeaverInitialEmotionalAwareness),
    (ZenWeaver, "acupuncture", ZenWeaverInitialArachnidAcupuncture),
    (BattlewornDummy, "setting_1", BattlewornDummyInitialSetting1),
    (BattlewornDummy, "setting_2", BattlewornDummyInitialSetting2),
    (BattlewornDummy, "setting_3", BattlewornDummyInitialSetting3),
    (Reflections, "shatter", ReflectionsInitialShatter),
    (Reflections, "touch_a_mirror", ReflectionsInitialTouchAMirror),
    (TinkerTime, "choose_card_type", TinkerTimeInitialChooseCardType),
    (TinkerTime, "attack", TinkerTimeChooseCardTypeAttack),
    (TinkerTime, "skill", TinkerTimeChooseCardTypeSkill),
    (TinkerTime, "power", TinkerTimeChooseCardTypePower),
    (TinkerTime, "sapping", TinkerTimeChooseRiderSapping),
    (TinkerTime, "violence", TinkerTimeChooseRiderViolence),
    (TinkerTime, "choking", TinkerTimeChooseRiderChoking),
    (TinkerTime, "energized", TinkerTimeChooseRiderEnergized),
    (TinkerTime, "wisdom", TinkerTimeChooseRiderWisdom),
    (TinkerTime, "chaos", TinkerTimeChooseRiderChaos),
    (TinkerTime, "expertise", TinkerTimeChooseRiderExpertise),
    (TinkerTime, "curious", TinkerTimeChooseRiderCurious),
    (TinkerTime, "improvement", TinkerTimeChooseRiderImprovement),
    (Trial, "accept", TrialInitialAccept),
    (Trial, "reject", TrialInitialReject),
    (Trial, "double_down", TrialRejectDoubleDown),
    (WarHistorianRepy, "unlock_cage", WarHistorianRepyInitialUnlockCage),
    (WarHistorianRepy, "unlock_chest", WarHistorianRepyInitialUnlockChest),
}

impl EventOptionIdentity {
    pub const fn neow(slot: u16) -> Self {
        assert!(slot < 3);
        Self { id: EventOptionId::NeowPageNameRelicIdEntry, parameter: EventParameter::Offer(slot) }
    }
    fn from_legacy(event: EventId, page: EventPageIdentity, key: &str) -> Self {
        use EventOptionId as O;
        use EventParameter as P;
        if event==EventId::Trial {
            if matches!(key,"confirm_abandon"|"cancel_abandon") {
                return Self{id:O::TrialRejectDoubleDown,parameter:P::AbandonRunConfirmation(key=="confirm_abandon")};
            }
            let id=match (page.id,key) {
                (EventPageId::TrialReject,"accept")=>Some(O::TrialRejectAccept),
                (EventPageId::TrialMerchant,"guilty")=>Some(O::TrialMerchantGuilty),
                (EventPageId::TrialMerchant,"innocent")=>Some(O::TrialMerchantInnocent),
                (EventPageId::TrialNoble,"guilty")=>Some(O::TrialNobleGuilty),
                (EventPageId::TrialNoble,"innocent")=>Some(O::TrialNobleInnocent),
                (EventPageId::TrialNondescript,"guilty")=>Some(O::TrialNondescriptGuilty),
                (EventPageId::TrialNondescript,"innocent")=>Some(O::TrialNondescriptInnocent),
                _=>None,
            };
            if let Some(id)=id{return Self{id,parameter:P::None};}
        }
        if let Some(id) = fixed_from_legacy(event, key) {
            let parameter = if id == O::TheArchitectDialogueCurrentLineIndex { page.parameter } else { P::None };
            return Self { id, parameter };
        }
        let indexed = match event {
            EventId::Tanx => Some((O::TanxPageNameRelicIdEntry, &["relic_1", "relic_2", "relic_3"][..])),
            EventId::Darv => Some((O::DarvPageNameRelicIdEntry, &DARV_MENU_KEYS[..])),
            EventId::Nonupeipe => Some((O::NonupeipePageNameRelicIdEntry, &["relic_1", "relic_2", "relic_3"][..])),
            EventId::Orobas => Some((O::OrobasPageNameRelicIdEntry, &["relic_1", "relic_2", "relic_3"][..])),
            EventId::Pael => Some((O::PaelPageNameRelicIdEntry, &["relic_1", "relic_2", "relic_3"][..])),
            EventId::Vakuu => Some((O::VakuuPageNameRelicIdEntry, &["relic_1", "relic_2", "relic_3"][..])),
            EventId::Tezcatara => Some((O::TezcataraPageNameRelicIdEntry,
                &["comfort_1", "comfort_2", "comfort_3", "wax_0", "wax_1", "wax_2", "wax_3", "wax_4", "skip_wax"][..])),
            EventId::ColorfulPhilosophers => Some((O::ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant, &["pool_1", "pool_2", "pool_3"][..])),
            EventId::DollRoom => Some((O::DollRoomTakeTake, &["doll_1", "doll_2", "doll_3"][..])),
            EventId::TheFutureOfPotions => Some((O::TheFutureOfPotionsInitialPotion, &["trade_0", "trade_1", "trade_2"][..])),
            EventId::FakeMerchant => Some((O::FakeMerchantNFakeMerchantMerchantRelicEntry, &["buy_0", "buy_1", "buy_2", "buy_3", "buy_4", "buy_5"][..])),
            _ => None,
        };
        if let Some((id, keys)) = indexed {
            let slot = keys.iter().position(|&candidate| candidate == key).expect("invalid indexed event option");
            return Self { id, parameter: P::Offer(slot as u16) };
        }
        let (id, parameter) = match (event, key) {
            (EventId::HungryForMushrooms, "big") => (O::HungryForMushroomsPageNameRelicIdEntry, P::Relic(RelicId::BigMushroom)),
            (EventId::HungryForMushrooms, "fragrant") => (O::HungryForMushroomsPageNameRelicIdEntry, P::Relic(RelicId::FragrantMushroom)),
            (EventId::SlipperyBridge, "hold_on") => if page.id == EventPageId::SlipperyBridgeInitial {
                (O::SlipperyBridgeInitialHoldOn0, P::Bridge(Some(0)))
            } else {
                let P::Bridge(previous) = page.parameter else { panic!("bridge page suffix missing") };
                (O::SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2,
                    P::Bridge(previous.and_then(|count| (count + 1 < 7).then_some(count + 1))))
            },
            (EventId::TabletOfTruth, "decipher") => if page.id == EventPageId::TabletOfTruthInitial {
                (O::TabletOfTruthInitialDecipher1, P::Counter(1))
            } else { (O::TabletOfTruthDecipherDecipherCountDecipher, page.parameter) },
            (EventId::ColossalFlower, "extract") => match page.id {
                EventPageId::ColossalFlowerInitial => (O::ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1, P::Counter(1)),
                EventPageId::ColossalFlowerReachDeeper2 => (O::ColossalFlowerReachDeeper2ExtractInstead, P::None),
                _ => (O::ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1, P::Counter(2)),
            },
            (EventId::ColossalFlower, "reach_deeper") => if page.id == EventPageId::ColossalFlowerInitial {
                (O::ColossalFlowerInitialReachDeeperNumberOfDigs1, P::Counter(1))
            } else { (O::ColossalFlowerReachDeeperNumberOfDigsReachDeeperNumberOfDigs1, P::Counter(2)) },
            (EventId::ColossalFlower, "pollinous_core") => (O::ColossalFlowerReachDeeper2PollinousCore, P::None),
            _ => panic!("event option lacks an original-source identity: {event:?}/{key}"),
        };
        Self { id, parameter }
    }
    /// Boundary for existing RunAction and original replay adapters.
    pub(crate) fn legacy_id(self) -> &'static str {
        use EventOptionId as O;
        use EventParameter as P;
        if let P::AbandonRunConfirmation(confirm)=self.parameter {
            return if confirm {"confirm_abandon"}else{"cancel_abandon"};
        }
        if let P::SphereReward{offer,action}=self.parameter {
            return super::sphere_reward_option_id(usize::from(offer),usize::from(action));
        }
        if let Some(key) = fixed_to_legacy(self.id) { return key; }
        match (self.id, self.parameter) {
            (O::TrialRejectAccept,_)=>"accept",
            (O::TrialMerchantGuilty|O::TrialNobleGuilty|O::TrialNondescriptGuilty,_)=>"guilty",
            (O::TrialMerchantInnocent|O::TrialNobleInnocent|O::TrialNondescriptInnocent,_)=>"innocent",
            (O::NeowPageNameRelicIdEntry, P::Offer(i)) => ["positive_1", "positive_2", "cursed"][i as usize],
            (O::DarvPageNameRelicIdEntry, P::Offer(i)) => DARV_MENU_KEYS[i as usize],
            (O::NonupeipePageNameRelicIdEntry |
                O::OrobasPageNameRelicIdEntry | O::PaelPageNameRelicIdEntry |
                O::VakuuPageNameRelicIdEntry | O::TanxPageNameRelicIdEntry, P::Offer(i)) => ["relic_1", "relic_2", "relic_3"][i as usize],
            (O::TezcataraPageNameRelicIdEntry, P::Offer(i)) =>
                ["comfort_1", "comfort_2", "comfort_3", "wax_0", "wax_1", "wax_2", "wax_3", "wax_4", "skip_wax"][i as usize],
            (O::ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant, P::Offer(i)) => ["pool_1", "pool_2", "pool_3"][i as usize],
            (O::DollRoomTakeTake, P::Offer(i)) => ["doll_1", "doll_2", "doll_3"][i as usize],
            (O::TheFutureOfPotionsInitialPotion, P::Offer(i)) => ["trade_0", "trade_1", "trade_2"][i as usize],
            (O::FakeMerchantNFakeMerchantMerchantRelicEntry, P::Offer(i)) => ["buy_0", "buy_1", "buy_2", "buy_3", "buy_4", "buy_5"][i as usize],
            (O::HungryForMushroomsPageNameRelicIdEntry, P::Relic(RelicId::BigMushroom)) => "big",
            (O::HungryForMushroomsPageNameRelicIdEntry, P::Relic(RelicId::FragrantMushroom)) => "fragrant",
            (O::SlipperyBridgeInitialHoldOn0 | O::SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2, _) => "hold_on",
            (O::TabletOfTruthInitialDecipher1 | O::TabletOfTruthDecipherDecipherCountDecipher, _) => "decipher",
            (O::ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1 |
                O::ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1 |
                O::ColossalFlowerReachDeeper2ExtractInstead, _) => "extract",
            (O::ColossalFlowerInitialReachDeeperNumberOfDigs1 |
                O::ColossalFlowerReachDeeperNumberOfDigsReachDeeperNumberOfDigs1, _) => "reach_deeper",
            (O::ColossalFlowerReachDeeper2PollinousCore, _) => "pollinous_core",
            (O::CrystalSphereCrystalSphereMinigameCellClicked, P::Sphere { big, x, y }) => super::sphere_option_id(big, x as usize, y as usize),
            _ => panic!("invalid typed event option parameter"),
        }
    }

    #[inline]
    pub const fn is(self, id: EventOptionId) -> bool {
        self.id as u16 == id as u16
    }

    #[inline]
    pub const fn offer_index(self, id: EventOptionId) -> Option<usize> {
        if !self.is(id) {
            return None;
        }
        match self.parameter {
            EventParameter::Offer(slot) => Some(slot as usize),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEventOption {
    pub option_id: EventOptionIdentity,
    pub label: &'static str,
    pub description: &'static str,
    pub dynamic_description: Option<EventDescription>,
    pub enabled: bool,
}
impl NativeEventOption {
    pub(crate) fn sphere(big: bool, x: u8, y: u8) -> Self {
        assert!(x < 11 && y < 11);
        Self { option_id: EventOptionIdentity { id: EventOptionId::CrystalSphereCrystalSphereMinigameCellClicked,
                parameter: EventParameter::Sphere { big, x, y } },
            label: super::sphere_option_label(big, x as usize, y as usize),
            description: "", dynamic_description: None, enabled: true }
    }
    /// Enablement can change after inventory actions, so resolve locked IDs from live state.
    pub fn native_id(&self) -> EventOptionId {
        use EventOptionId as O;
        if self.enabled { return self.option_id.id; }
        match self.option_id.id {
            O::EndlessConveyorAllId => O::EndlessConveyorAllLocked,
            O::GraveOfTheForgottenInitialConfront => O::GraveOfTheForgottenInitialConfrontLocked,
            O::LuminousChoirInitialOfferTribute => O::LuminousChoirInitialOfferTributeLocked,
            O::OrobasPageNameRelicIdEntry => O::OrobasInitialOptionPool3Locked,
            O::RanwidTheElderInitialPotion => O::RanwidTheElderInitialPotionLocked,
            O::RanwidTheElderInitialRelic => O::RanwidTheElderInitialRelicLocked,
            O::SelfHelpBookInitialReadTheBack => O::SelfHelpBookInitialReadTheBackLocked,
            O::SelfHelpBookInitialReadPassage => O::SelfHelpBookInitialReadPassageLocked,
            O::SelfHelpBookInitialReadEntireBook => O::SelfHelpBookInitialReadEntireBookLocked,
            O::StoneOfAllTimeInitialLift => O::StoneOfAllTimeInitialLiftLocked,
            O::StoneOfAllTimeInitialPush => O::StoneOfAllTimeInitialPushLocked,
            O::SymbioteInitialApproach => O::SymbioteInitialApproachLocked,
            O::TeaMasterInitialBoneTea => O::TeaMasterInitialBoneTeaLocked,
            O::TeaMasterInitialEmberTea => O::TeaMasterInitialEmberTeaLocked,
            O::WaterloggedScriptoriumInitialTentacleQuill => O::WaterloggedScriptoriumInitialTentacleQuillLocked,
            O::WaterloggedScriptoriumInitialPricklySponge => O::WaterloggedScriptoriumInitialPricklySpongeLocked,
            O::WelcomeToWongosInitialBargainBin => O::WelcomeToWongosInitialBargainBinLocked,
            O::WelcomeToWongosInitialFeaturedItem => O::WelcomeToWongosInitialFeaturedItemLocked,
            O::WelcomeToWongosInitialMysteryBox => O::WelcomeToWongosInitialMysteryBoxLocked,
            O::WoodCarvingsInitialSnake => O::WoodCarvingsInitialSnakeLocked,
            O::ZenWeaverInitialEmotionalAwareness | O::ZenWeaverInitialArachnidAcupuncture => O::ZenWeaverInitialLocked,
            id => id,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EventOptions {
    event: EventId,
    pub page: EventPageIdentity,
    items: Vec<NativeEventOption>,
}
impl EventOptions {
    pub(crate) fn new(event: EventId) -> Self {
        let (id, parameter) = match event {
            EventId::FakeMerchant => (EventPageId::FakeMerchantNFakeMerchant, EventParameter::None),
            EventId::TheArchitect => (EventPageId::TheArchitectDialogueCurrentLineIndex, EventParameter::Counter(0)),
            _ => (event.initial_page(), EventParameter::None),
        };
        Self { event, page: EventPageIdentity { id, parameter }, items: Vec::new() }
    }
    pub(crate) fn set_page(&mut self, id: EventPageId, parameter: EventParameter) {
        assert_eq!(id.event(), self.event, "page belongs to another event");
        self.page = EventPageIdentity { id, parameter };
    }
    pub(crate) fn push(&mut self, template: EventOption) {
        let option_id = EventOptionIdentity::from_legacy(self.event, self.page, template.option_id);
        self.items.push(NativeEventOption { option_id, label: template.label,
            description: template.description, dynamic_description: template.dynamic_description, enabled: template.enabled });
    }
    pub(crate) fn clear(&mut self) { self.items.clear(); }
    pub(crate) fn remove(&mut self, index: usize) -> NativeEventOption { self.items.remove(index) }
    pub(crate) fn replace(&mut self, templates: impl IntoIterator<Item = EventOption>) {
        self.clear();
        for template in templates { self.push(template); }
    }
    pub(crate) fn replace_ids(
        &mut self,
        items: impl IntoIterator<Item = (EventOptionId, &'static str, &'static str)>,
    ) {
        self.clear();
        self.items.extend(items.into_iter().map(|(id, label, description)| NativeEventOption {
            option_id: EventOptionIdentity { id, parameter: EventParameter::None },
            label,
            description,
            dynamic_description: None,
            enabled: true,
        }));
    }
    pub(crate) fn replace_sphere(&mut self, items: Vec<NativeEventOption>) {
        assert_eq!(self.event, EventId::CrystalSphere);
        assert!(items.iter().all(|item| item.option_id.id == EventOptionId::CrystalSphereCrystalSphereMinigameCellClicked));
        self.set_page(EventPageId::CrystalSphereCrystalSphereMinigame, EventParameter::None);
        self.items = items;
    }
    pub(crate) fn push_sphere_reward(&mut self, offer:usize, action:usize, label:&'static str) {
        assert_eq!(self.event,EventId::CrystalSphere);
        assert!(offer<150 && action<7);
        let parameter=EventParameter::SphereReward{offer:offer as u16,action:action as u8};
        self.page.parameter=parameter;
        self.items.push(NativeEventOption{
            // Carrier for the parent minigame command, never published as a
            // native reward ID: the native encoder refuses this modal phase.
            option_id:EventOptionIdentity{id:EventOptionId::CrystalSphereCrystalSphereMinigameCellClicked,parameter},
            label,description:"",dynamic_description:None,enabled:true,
        });
    }
}
impl core::ops::Deref for EventOptions {
    type Target = [NativeEventOption];
    fn deref(&self) -> &Self::Target { &self.items }
}
impl core::ops::DerefMut for EventOptions {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.items }
}
impl<'a> IntoIterator for &'a EventOptions {
    type Item = &'a NativeEventOption;
    type IntoIter = core::slice::Iter<'a, NativeEventOption>;
    fn into_iter(self) -> Self::IntoIter { self.items.iter() }
}
impl<'a> IntoIterator for &'a mut EventOptions {
    type Item = &'a mut NativeEventOption;
    type IntoIter = core::slice::IterMut<'a, NativeEventOption>;
    fn into_iter(self) -> Self::IntoIter { self.items.iter_mut() }
}

