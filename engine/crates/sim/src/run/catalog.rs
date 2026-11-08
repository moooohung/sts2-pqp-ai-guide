use crate::identity::{EncounterId, EventId};
// Generated once from frozen r7 registry/catalog; order is gameplay/RNG-significant.

#[derive(Clone, Copy, Debug)]
pub struct EncounterDef {
    pub id: EncounterId,
    pub tags: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ActDef {
    pub name: &'static str,
    pub num_rooms: usize,
    pub weak_count: usize,
    pub events: &'static [EventId],
    pub weak: &'static [EncounterDef],
    pub normal: &'static [EncounterDef],
    pub elite: &'static [EncounterDef],
    pub bosses: &'static [EncounterId],
    pub ancients: &'static [EventId],
}

pub static ACT0_EVENTS: &[EventId] = &[
    EventId::AromaOfChaos,
    EventId::ByrdonisNest,
    EventId::DenseVegetation,
    EventId::JungleMazeAdventure,
    EventId::LuminousChoir,
    EventId::MorphicGrove,
    EventId::SapphireSeed,
    EventId::SunkenStatue,
    EventId::TabletOfTruth,
    EventId::UnrestSite,
    EventId::Wellspring,
    EventId::WhisperingHollow,
    EventId::WoodCarvings,
    EventId::BrainLeech,
    EventId::CrystalSphere,
    EventId::DollRoom,
    EventId::FakeMerchant,
    EventId::PotionCourier,
    EventId::RanwidTheElder,
    EventId::RelicTrader,
    EventId::RoomFullOfCheese,
    EventId::SelfHelpBook,
    EventId::SlipperyBridge,
    EventId::StoneOfAllTime,
    EventId::Symbiote,
    EventId::TeaMaster,
    EventId::TheFutureOfPotions,
    EventId::TheLegendsWereTrue,
    EventId::ThisOrThat,
    EventId::WarHistorianRepy,
    EventId::WelcomeToWongos,
];

pub static ACT0_WEAK: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::FuzzyWurmCrawlerWeak,
        tags: 512u64,
    },
    EncounterDef {
        id: EncounterId::NibbitsWeak,
        tags: 1024u64,
    },
    EncounterDef {
        id: EncounterId::ShrinkerBeetleWeak,
        tags: 2048u64,
    },
    EncounterDef {
        id: EncounterId::SlimesWeak,
        tags: 256u64,
    },
];

pub static ACT0_NORMAL: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::CubexConstructNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::FlyconidNormal,
        tags: 384u64,
    },
    EncounterDef {
        id: EncounterId::FogmogNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::InkletsNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::MawlerNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::NibbitsNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::OvergrowthCrawlers,
        tags: 2560u64,
    },
    EncounterDef {
        id: EncounterId::RubyRaidersNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::SlimesNormal,
        tags: 256u64,
    },
    EncounterDef {
        id: EncounterId::SlitheringStranglerNormal,
        tags: 4352u64,
    },
    EncounterDef {
        id: EncounterId::SnappingJaxfruitNormal,
        tags: 4224u64,
    },
    EncounterDef {
        id: EncounterId::VineShamblerNormal,
        tags: 0u64,
    },
];

pub static ACT0_ELITE: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::BygoneEffigyElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::ByrdonisElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::PhrogParasiteElite,
        tags: 0u64,
    },
];

pub static ACT0_BOSSES: &[EncounterId] = &[
    EncounterId::CeremonialBeastBoss,
    EncounterId::TheKinBoss,
    EncounterId::VantomBoss,
];

pub static ACT0_ANCIENTS: &[EventId] = &[EventId::Neow];

pub static ACT0: ActDef = ActDef {
    name: "Overgrowth",
    num_rooms: 15,
    weak_count: 3,
    events: ACT0_EVENTS,
    weak: ACT0_WEAK,
    normal: ACT0_NORMAL,
    elite: ACT0_ELITE,
    bosses: ACT0_BOSSES,
    ancients: ACT0_ANCIENTS,
};

pub static ACT1_EVENTS: &[EventId] = &[
    EventId::AbyssalBaths,
    EventId::DrowningBeacon,
    EventId::EndlessConveyor,
    EventId::PunchOff,
    EventId::SpiralingWhirlpool,
    EventId::SunkenStatue,
    EventId::SunkenTreasury,
    EventId::DoorsOfLightAndDark,
    EventId::TrashHeap,
    EventId::WaterloggedScriptorium,
    EventId::BrainLeech,
    EventId::CrystalSphere,
    EventId::DollRoom,
    EventId::FakeMerchant,
    EventId::PotionCourier,
    EventId::RanwidTheElder,
    EventId::RelicTrader,
    EventId::RoomFullOfCheese,
    EventId::SelfHelpBook,
    EventId::SlipperyBridge,
    EventId::StoneOfAllTime,
    EventId::Symbiote,
    EventId::TeaMaster,
    EventId::TheFutureOfPotions,
    EventId::TheLegendsWereTrue,
    EventId::ThisOrThat,
    EventId::WarHistorianRepy,
    EventId::WelcomeToWongos,
];

pub static ACT1_WEAK: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::CorpseSlugsWeak,
        tags: 8192u64,
    },
    EncounterDef {
        id: EncounterId::SeapunkWeak,
        tags: 16384u64,
    },
    EncounterDef {
        id: EncounterId::SludgeSpinnerWeak,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::ToadpolesWeak,
        tags: 0u64,
    },
];

pub static ACT1_NORMAL: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::CorpseSlugsNormal,
        tags: 8192u64,
    },
    EncounterDef {
        id: EncounterId::CultistsNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::FossilStalkerNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::GremlinMercNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::HauntedShipNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::LivingFogNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::PunchConstructNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::SeapunkNormal,
        tags: 16384u64,
    },
    EncounterDef {
        id: EncounterId::SewerClamNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::TwoTailedRatsNormal,
        tags: 0u64,
    },
];

pub static ACT1_ELITE: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::PhantasmalGardenersElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::SkulkingColonyElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::TerrorEelElite,
        tags: 0u64,
    },
];

pub static ACT1_BOSSES: &[EncounterId] = &[
    EncounterId::LagavulinMatriarchBoss,
    EncounterId::SoulFyshBoss,
    EncounterId::WaterfallGiantBoss,
];

pub static ACT1_ANCIENTS: &[EventId] = &[EventId::Neow];

pub static ACT1: ActDef = ActDef {
    name: "Underdocks",
    num_rooms: 15,
    weak_count: 3,
    events: ACT1_EVENTS,
    weak: ACT1_WEAK,
    normal: ACT1_NORMAL,
    elite: ACT1_ELITE,
    bosses: ACT1_BOSSES,
    ancients: ACT1_ANCIENTS,
};

pub static ACT2_EVENTS: &[EventId] = &[
    EventId::Amalgamator,
    EventId::Bugslayer,
    EventId::ColorfulPhilosophers,
    EventId::ColossalFlower,
    EventId::FieldOfManSizedHoles,
    EventId::InfestedAutomaton,
    EventId::LostWisp,
    EventId::SpiritGrafter,
    EventId::TheLanternKey,
    EventId::ZenWeaver,
    EventId::BrainLeech,
    EventId::CrystalSphere,
    EventId::DollRoom,
    EventId::FakeMerchant,
    EventId::PotionCourier,
    EventId::RanwidTheElder,
    EventId::RelicTrader,
    EventId::RoomFullOfCheese,
    EventId::SelfHelpBook,
    EventId::SlipperyBridge,
    EventId::StoneOfAllTime,
    EventId::Symbiote,
    EventId::TeaMaster,
    EventId::TheFutureOfPotions,
    EventId::TheLegendsWereTrue,
    EventId::ThisOrThat,
    EventId::WarHistorianRepy,
    EventId::WelcomeToWongos,
];

pub static ACT2_WEAK: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::BowlbugsWeak,
        tags: 4u64,
    },
    EncounterDef {
        id: EncounterId::ExoskeletonsWeak,
        tags: 16u64,
    },
    EncounterDef {
        id: EncounterId::ThievingHopperWeak,
        tags: 32u64,
    },
    EncounterDef {
        id: EncounterId::TunnelerWeak,
        tags: 64u64,
    },
];

pub static ACT2_NORMAL: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::BowlbugsNormal,
        tags: 4u64,
    },
    EncounterDef {
        id: EncounterId::ChompersNormal,
        tags: 8u64,
    },
    EncounterDef {
        id: EncounterId::ExoskeletonsNormal,
        tags: 16u64,
    },
    EncounterDef {
        id: EncounterId::HunterKillerNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::LouseProgenitorNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::MytesNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::OvicopterNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::SlumberingBeetleNormal,
        tags: 4u64,
    },
    EncounterDef {
        id: EncounterId::SpinyToadNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::TheObscuraNormal,
        tags: 0u64,
    },
];

pub static ACT2_ELITE: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::DecimillipedeElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::EntomancerElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::InfestedPrismsElite,
        tags: 0u64,
    },
];

pub static ACT2_BOSSES: &[EncounterId] = &[
    EncounterId::KaiserCrabBoss,
    EncounterId::KnowledgeDemonBoss,
    EncounterId::TheInsatiableBoss,
];

pub static ACT2_ANCIENTS: &[EventId] = &[EventId::Orobas, EventId::Pael, EventId::Tezcatara];

pub static ACT2: ActDef = ActDef {
    name: "Hive",
    num_rooms: 14,
    weak_count: 2,
    events: ACT2_EVENTS,
    weak: ACT2_WEAK,
    normal: ACT2_NORMAL,
    elite: ACT2_ELITE,
    bosses: ACT2_BOSSES,
    ancients: ACT2_ANCIENTS,
};

pub static ACT3_EVENTS: &[EventId] = &[
    EventId::BattlewornDummy,
    EventId::GraveOfTheForgotten,
    EventId::HungryForMushrooms,
    EventId::Reflections,
    EventId::RoundTeaParty,
    EventId::Trial,
    EventId::TinkerTime,
    EventId::BrainLeech,
    EventId::CrystalSphere,
    EventId::DollRoom,
    EventId::FakeMerchant,
    EventId::PotionCourier,
    EventId::RanwidTheElder,
    EventId::RelicTrader,
    EventId::RoomFullOfCheese,
    EventId::SelfHelpBook,
    EventId::SlipperyBridge,
    EventId::StoneOfAllTime,
    EventId::Symbiote,
    EventId::TeaMaster,
    EventId::TheFutureOfPotions,
    EventId::TheLegendsWereTrue,
    EventId::ThisOrThat,
    EventId::WarHistorianRepy,
    EventId::WelcomeToWongos,
];

pub static ACT3_WEAK: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::DevotedSculptorWeak,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::ScrollsOfBitingWeak,
        tags: 2u64,
    },
    EncounterDef {
        id: EncounterId::TurretOperatorWeak,
        tags: 0u64,
    },
];

pub static ACT3_NORMAL: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::AxebotsNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::ConstructMenagerieNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::FabricatorNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::FrogKnightNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::GlobeHeadNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::OwlMagistrateNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::ScrollsOfBitingNormal,
        tags: 2u64,
    },
    EncounterDef {
        id: EncounterId::SlimedBerserkerNormal,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::TheLostAndForgottenNormal,
        tags: 0u64,
    },
];

pub static ACT3_ELITE: &[EncounterDef] = &[
    EncounterDef {
        id: EncounterId::KnightsElite,
        tags: 1u64,
    },
    EncounterDef {
        id: EncounterId::MechaKnightElite,
        tags: 0u64,
    },
    EncounterDef {
        id: EncounterId::SoulNexusElite,
        tags: 0u64,
    },
];

pub static ACT3_BOSSES: &[EncounterId] = &[
    EncounterId::AeonglassBoss,
    EncounterId::QueenBoss,
    EncounterId::TestSubjectBoss,
];

pub static ACT3_ANCIENTS: &[EventId] = &[EventId::Nonupeipe, EventId::Tanx, EventId::Vakuu];

pub static ACT3: ActDef = ActDef {
    name: "Glory",
    num_rooms: 13,
    weak_count: 2,
    events: ACT3_EVENTS,
    weak: ACT3_WEAK,
    normal: ACT3_NORMAL,
    elite: ACT3_ELITE,
    bosses: ACT3_BOSSES,
    ancients: ACT3_ANCIENTS,
};

pub static ACTS: [&ActDef; 4] = [&ACT0, &ACT1, &ACT2, &ACT3];

pub static SHARED_ANCIENTS: &[EventId] = &[EventId::Darv];

pub static SHARED_RELICS_COMMON: &[&str] = &[
    "AMETHYST_AUBERGINE",
    "ANCHOR",
    "BAG_OF_MARBLES",
    "BAG_OF_PREPARATION",
    "BLOOD_VIAL",
    "BOOK_OF_FIVE_RINGS",
    "BRONZE_SCALES",
    "CENTENNIAL_PUZZLE",
    "FESTIVE_POPPER",
    "GORGET",
    "HAPPY_FLOWER",
    "JUZU_BRACELET",
    "LANTERN",
    "MEAL_TICKET",
    "ODDLY_SMOOTH_STONE",
    "PENDULUM",
    "POTION_BELT",
    "RED_MASK",
    "REGAL_PILLOW",
    "STRAWBERRY",
    "STRIKE_DUMMY",
    "VAJRA",
    "VENERABLE_TEA_SET",
    "WAR_PAINT",
    "WHETSTONE",
];

pub static IRON_RELICS_COMMON: &[&str] = &[
    "AMETHYST_AUBERGINE",
    "ANCHOR",
    "BAG_OF_MARBLES",
    "BAG_OF_PREPARATION",
    "BLOOD_VIAL",
    "BOOK_OF_FIVE_RINGS",
    "BRONZE_SCALES",
    "CENTENNIAL_PUZZLE",
    "FESTIVE_POPPER",
    "GORGET",
    "HAPPY_FLOWER",
    "JUZU_BRACELET",
    "LANTERN",
    "MEAL_TICKET",
    "ODDLY_SMOOTH_STONE",
    "PENDULUM",
    "POTION_BELT",
    "RED_MASK",
    "REGAL_PILLOW",
    "STRAWBERRY",
    "STRIKE_DUMMY",
    "VAJRA",
    "VENERABLE_TEA_SET",
    "WAR_PAINT",
    "WHETSTONE",
    "RED_SKULL",
];

pub static SHARED_RELICS_UNCOMMON: &[&str] = &[
    "AKABEKO",
    "BOWLER_HAT",
    "CANDELABRA",
    "ETERNAL_FEATHER",
    "GREMLIN_HORN",
    "HORN_CLEAT",
    "JOSS_PAPER",
    "KUSARIGAMA",
    "LASTING_CANDY",
    "LETTER_OPENER",
    "LUCKY_FYSH",
    "MERCURY_HOURGLASS",
    "MINIATURE_CANNON",
    "NUNCHAKU",
    "ORICHALCUM",
    "ORNAMENTAL_FAN",
    "PANTOGRAPH",
    "PARRYING_SHIELD",
    "PEAR",
    "PEN_NIB",
    "PERMAFROST",
    "PETRIFIED_TOAD",
    "PLANISPHERE",
    "REPTILE_TRINKET",
    "RIPPLE_BASIN",
    "SPARKLING_ROUGE",
    "STONE_CRACKER",
    "TINY_MAILBOX",
    "TUNING_FORK",
    "VAMBRACE",
];

pub static IRON_RELICS_UNCOMMON: &[&str] = &[
    "AKABEKO",
    "BOWLER_HAT",
    "CANDELABRA",
    "ETERNAL_FEATHER",
    "GREMLIN_HORN",
    "HORN_CLEAT",
    "JOSS_PAPER",
    "KUSARIGAMA",
    "LASTING_CANDY",
    "LETTER_OPENER",
    "LUCKY_FYSH",
    "MERCURY_HOURGLASS",
    "MINIATURE_CANNON",
    "NUNCHAKU",
    "ORICHALCUM",
    "ORNAMENTAL_FAN",
    "PANTOGRAPH",
    "PARRYING_SHIELD",
    "PEAR",
    "PEN_NIB",
    "PERMAFROST",
    "PETRIFIED_TOAD",
    "PLANISPHERE",
    "REPTILE_TRINKET",
    "RIPPLE_BASIN",
    "SPARKLING_ROUGE",
    "STONE_CRACKER",
    "TINY_MAILBOX",
    "TUNING_FORK",
    "VAMBRACE",
    "PAPER_PHROG",
    "SELF_FORMING_CLAY",
];

pub static SHARED_RELICS_RARE: &[&str] = &[
    "ART_OF_WAR",
    "BEATING_REMNANT",
    "BELLOWS",
    "CAPTAINS_WHEEL",
    "CHANDELIER",
    "CLOAK_CLASP",
    "FROZEN_EGG",
    "GAMBLING_CHIP",
    "GAME_PIECE",
    "GIRYA",
    "ICE_CREAM",
    "INTIMIDATING_HELMET",
    "KUNAI",
    "LIZARD_TAIL",
    "MANGO",
    "MEAT_ON_THE_BONE",
    "MOLTEN_EGG",
    "MUMMIFIED_HAND",
    "OLD_COIN",
    "POCKETWATCH",
    "PRAYER_WHEEL",
    "RAINBOW_RING",
    "RAZOR_TOOTH",
    "SHOVEL",
    "SHURIKEN",
    "STONE_CALENDAR",
    "STURDY_CLAMP",
    "THE_COURIER",
    "TOXIC_EGG",
    "TUNGSTEN_ROD",
    "UNCEASING_TOP",
    "UNSETTLING_LAMP",
    "VEXING_PUZZLEBOX",
    "WHITE_BEAST_STATUE",
    "WHITE_STAR",
];

pub static IRON_RELICS_RARE: &[&str] = &[
    "ART_OF_WAR",
    "BEATING_REMNANT",
    "BELLOWS",
    "CAPTAINS_WHEEL",
    "CHANDELIER",
    "CLOAK_CLASP",
    "FROZEN_EGG",
    "GAMBLING_CHIP",
    "GAME_PIECE",
    "GIRYA",
    "ICE_CREAM",
    "INTIMIDATING_HELMET",
    "KUNAI",
    "LIZARD_TAIL",
    "MANGO",
    "MEAT_ON_THE_BONE",
    "MOLTEN_EGG",
    "MUMMIFIED_HAND",
    "OLD_COIN",
    "POCKETWATCH",
    "PRAYER_WHEEL",
    "RAINBOW_RING",
    "RAZOR_TOOTH",
    "SHOVEL",
    "SHURIKEN",
    "STONE_CALENDAR",
    "STURDY_CLAMP",
    "THE_COURIER",
    "TOXIC_EGG",
    "TUNGSTEN_ROD",
    "UNCEASING_TOP",
    "UNSETTLING_LAMP",
    "VEXING_PUZZLEBOX",
    "WHITE_BEAST_STATUE",
    "WHITE_STAR",
    "CHARONS_ASHES",
    "DEMON_TONGUE",
    "RUINED_HELMET",
];

pub static SHARED_RELICS_SHOP: &[&str] = &[
    "BELT_BUCKLE",
    "BREAD",
    "BURNING_STICKS",
    "CAULDRON",
    "CHEMICAL_X",
    "DINGY_RUG",
    "DOLLYS_MIRROR",
    "DRAGON_FRUIT",
    "GHOST_SEED",
    "GNARLED_HAMMER",
    "KIFUDA",
    "LAVA_LAMP",
    "LEES_WAFFLE",
    "MEMBERSHIP_CARD",
    "MINIATURE_TENT",
    "MYSTIC_LIGHTER",
    "ORRERY",
    "PUNCH_DAGGER",
    "RINGING_TRIANGLE",
    "ROYAL_STAMP",
    "SCREAMING_FLAGON",
    "SLING_OF_COURAGE",
    "THE_ABACUS",
    "TOOLBOX",
    "WING_CHARM",
];

pub static IRON_RELICS_SHOP: &[&str] = &[
    "BELT_BUCKLE",
    "BREAD",
    "BURNING_STICKS",
    "CAULDRON",
    "CHEMICAL_X",
    "DINGY_RUG",
    "DOLLYS_MIRROR",
    "DRAGON_FRUIT",
    "GHOST_SEED",
    "GNARLED_HAMMER",
    "KIFUDA",
    "LAVA_LAMP",
    "LEES_WAFFLE",
    "MEMBERSHIP_CARD",
    "MINIATURE_TENT",
    "MYSTIC_LIGHTER",
    "ORRERY",
    "PUNCH_DAGGER",
    "RINGING_TRIANGLE",
    "ROYAL_STAMP",
    "SCREAMING_FLAGON",
    "SLING_OF_COURAGE",
    "THE_ABACUS",
    "TOOLBOX",
    "WING_CHARM",
    "BRIMSTONE",
];
