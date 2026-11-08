use super::*;
use sts2_sim::run::{EventDescription as D, EventOptionIdentity, EventParameter, EventDish,
    event_preview::*};

fn context<'a>(option: EventOptionId, deck: &'a [Card]) -> PublicEventContext<'a> {
    use EventOptionId::*;
    let description = match option {
        DenseVegetationInitialTrudgeOn => Some(D::DenseVegetationTrudgeGold(77)),
        DenseVegetationInitialRest => Some(D::DenseVegetationRestHeal(24)),
        ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1 => Some(D::ColossalFlowerExtractGold(35)),
        ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1 => Some(D::ColossalFlowerExtractGold(75)),
        ColossalFlowerReachDeeper2ExtractInstead => Some(D::ColossalFlowerExtractGold(135)),
        ColossalFlowerInitialReachDeeperNumberOfDigs1 => Some(D::ColossalFlowerReachDeeperDamage(5)),
        ColossalFlowerReachDeeperNumberOfDigsReachDeeperNumberOfDigs1 => Some(D::ColossalFlowerReachDeeperDamage(6)),
        ColossalFlowerReachDeeper2PollinousCore => Some(D::ColossalFlowerPollinousCoreDamage(7)),
        FakeMerchantNFakeMerchantMerchantRelicEntry => Some(D::FakeMerchantCost(50)),
        JungleMazeAdventureInitialSoloQuest => Some(D::JungleSoloGold(150.25f64.to_bits())),
        JungleMazeAdventureInitialJoinForces => Some(D::JungleJoinGold(50.25f64.to_bits())),
        LostWispInitialSearch => Some(D::LostWispSearchGold(60)),
        SlipperyBridgeInitialHoldOn0 | SlipperyBridgeHoldOnHoldOnSuffixHoldOnHoldOnSuffix2 => Some(D::BridgeHoldOn(3)),
        SpiralingWhirlpoolInitialDrink => Some(D::SpiralingWhirlpoolHeal(26)),
        SunkenStatueInitialDiveIntoWater => Some(D::SunkenStatueDive(111)),
        SunkenTreasuryInitialFirstChest => Some(D::SunkenTreasurySmallGold(60)),
        SunkenTreasuryInitialSecondChest => Some(D::SunkenTreasuryLargeGold(333)),
        TabletOfTruthInitialDecipher1 | TabletOfTruthDecipherDecipherCountDecipher => Some(D::TabletDecipherCost(3)),
        ThisOrThatInitialPlain => Some(D::ThisOrThatPlainGold(55)),
        UnrestSiteInitialRest => Some(D::UnrestSiteHeal(20)),
        _ => None,
    };
    let dynamic = match option.event() {
        EventId::AbyssalBaths => PublicEventDynamic::Damage(3),
        EventId::CrystalSphere => PublicEventDynamic::Price(71),
        EventId::LuminousChoir => PublicEventDynamic::Price(125),
        EventId::WhisperingHollow => PublicEventDynamic::Price(35),
        EventId::TabletOfTruth | EventId::WarHistorianRepy => PublicEventDynamic::Stage(0),
        EventId::TinkerTime => PublicEventDynamic::CardType(match option {
            TinkerTimeChooseRiderEnergized | TinkerTimeChooseRiderWisdom | TinkerTimeChooseRiderChaos => CardType::Skill,
            TinkerTimeChooseRiderExpertise | TinkerTimeChooseRiderCurious | TinkerTimeChooseRiderImprovement => CardType::Power,
            _ => CardType::Attack,
        }),
        _ => PublicEventDynamic::None,
    };
    PublicEventContext { hp: 60, max_hp: 80, gold: 500, deck, players: 1, description, dynamic }
}

#[test]
fn every_original_event_option_emits_valid_public_cost_or_consequence_rows() {
    let original: serde_json::Value = serde_json::from_str(include_str!("../../data/original_ids.json")).unwrap();
    assert_eq!(original["options"].as_array().unwrap().len(), EventOptionId::COUNT);
    let deck = [Card::from_factory(CardId::LanternKey, false, 1).unwrap()];
    for option in EventOptionId::ALL {
        let identity = EventOptionIdentity { id: option, parameter: EventParameter::ConveyorDish(EventDish::Clam) };
        let parameters = public_event_parameters(identity, context(option, &deck)).unwrap_or_else(|e| panic!("{option:?}: {e}"));
        assert!(!parameters.is_empty(), "{option:?} has no displayed cost, benefit or continuation semantics");
        let mut encoder = NativeEncoder::new(); let mut obs = NativeObsV2::new();
        encoder.begin(&mut obs, DecisionId::default(), 1).unwrap();
        let global = NativeEncoder::row(&mut obs, Row { kind: RowKind::Global, ..Row::default() }).unwrap();
        let owner = NativeEncoder::row(&mut obs, Row { kind: RowKind::Seat, parent: global, ..Row::default() }).unwrap();
        let row = NativeEncoder::row(&mut obs, Row { kind: RowKind::EventOption, model: option.into(),
            owner, parent: global, ..Row::default() }).unwrap();
        encoder.event_parameters(&mut obs, row, owner, &parameters).unwrap_or_else(|e| panic!("{option:?}: {e:?}"));
        obs.finish(DecisionStatus::Waiting).unwrap(); let v = obs.view().unwrap();
        assert!(v.rows.kind.iter().enumerate().any(|(i, &kind)| kind == RowKind::EffectParameter && v.rows.parent[i] == row),
            "{option:?} was admitted without public semantics");
    }
}

#[test]
fn displayed_static_costs_benefits_and_conditional_rewards_match_csharp() {
    use EventOptionId::*;
    // Literal expectations from the corresponding original handlers, not the catalog.
    for (option, expected) in [
        (DrowningBeaconInitialClimb, vec![("MaxHpCost", 13), ("RelicsGain", 1)]),
        (FieldOfManSizedHolesInitialResist, vec![("CardsRemove", 2), ("CursesGain", 1)]),
        (MorphicGroveInitialGroup, vec![("GoldCost", 500), ("CardsTransform", 2)]),
        (TrialMerchantGuilty, vec![("CursesGain", 1), ("RelicsGain", 2)]),
        (TrialNobleInnocent, vec![("CursesGain", 1), ("GoldGain", 300)]),
        (ZenWeaverInitialBreathingTechniques, vec![("GoldCost", 50), ("CardsGain", 2)]),
        (WaterloggedScriptoriumInitialPricklySponge, vec![("GoldCost", 99), ("CardsEnchant", 2), ("EnchantmentAmount", 1)]),
        (WarHistorianRepyInitialUnlockChest, vec![("CardsRemove", 1), ("RelicsGain", 2), ("PotionsGain", 2)]),
    ] {
        let deck = [Card::from_factory(CardId::LanternKey, false, 1).unwrap()];
        let values = public_event_parameters(EventOptionIdentity { id: option, parameter: EventParameter::None },
            context(option, &deck)).unwrap();
        for (name, amount) in expected {
            assert!(values.iter().any(|p| p.variable == name && p.amount == amount), "{option:?}: {name}");
        }
    }
    let option = EventOptionIdentity { id: BattlewornDummyInitialSetting2, parameter: EventParameter::None };
    let values = public_event_parameters(option, context(option.id, &[])).unwrap();
    assert!(values.iter().any(|p| p.variable == "CardsUpgrade" && p.amount == 2 && p.timing == PublicEventTiming::AfterVictory));
}

#[test]
fn conveyor_semantics_use_only_the_displayed_dish_and_locked_offers_hide_benefits() {
    let identity = EventOptionIdentity { id: EventOptionId::EndlessConveyorAllId,
        parameter: EventParameter::ConveyorDish(EventDish::Fysh) };
    let values = public_event_parameters(identity, context(identity.id, &[])).unwrap();
    assert!(values.iter().any(|p| p.variable == "GoldGain" && p.amount == 75));
    assert!(!values.iter().any(|p| p.variable == "GoldCost"));
    let locked = EventOptionIdentity { id: EventOptionId::WelcomeToWongosInitialFeaturedItemLocked, parameter: EventParameter::None };
    let values = public_event_parameters(locked, context(locked.id, &[])).unwrap();
    assert_eq!(values, vec![PublicEventParameter::integer("GoldCost", 200)]);
}
