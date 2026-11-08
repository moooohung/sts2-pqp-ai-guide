//! Typed native identities shared by retained engine state and NativeObsV1.
//! String conversion is confined to ingestion, diagnostics and legacy snapshots.
include!(concat!(env!("OUT_DIR"), "/gen_native_ids.rs"));

macro_rules! encounter_ids {
    ($(($variant:ident, $name:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EncounterId { $($variant),+ }

        impl EncounterId {
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

encounter_ids! {
    (ShrinkerBeetleWeak, "SHRINKER_BEETLE_WEAK"),
    (FuzzyWurmCrawlerWeak, "FUZZY_WURM_CRAWLER_WEAK"),
    (NibbitsWeak, "NIBBITS_WEAK"),
    (SlimesWeak, "SLIMES_WEAK"),
    (CubexConstructNormal, "CUBEX_CONSTRUCT_NORMAL"),
    (FlyconidNormal, "FLYCONID_NORMAL"),
    (FogmogNormal, "FOGMOG_NORMAL"),
    (InkletsNormal, "INKLETS_NORMAL"),
    (MawlerNormal, "MAWLER_NORMAL"),
    (NibbitsNormal, "NIBBITS_NORMAL"),
    (OvergrowthCrawlers, "OVERGROWTH_CRAWLERS"),
    (RubyRaidersNormal, "RUBY_RAIDERS_NORMAL"),
    (SlimesNormal, "SLIMES_NORMAL"),
    (SlitheringStranglerNormal, "SLITHERING_STRANGLER_NORMAL"),
    (SnappingJaxfruitNormal, "SNAPPING_JAXFRUIT_NORMAL"),
    (VineShamblerNormal, "VINE_SHAMBLER_NORMAL"),
    (BygoneEffigyElite, "BYGONE_EFFIGY_ELITE"),
    (ByrdonisElite, "BYRDONIS_ELITE"),
    (PhrogParasiteElite, "PHROG_PARASITE_ELITE"),
    (VantomBoss, "VANTOM_BOSS"),
    (CeremonialBeastBoss, "CEREMONIAL_BEAST_BOSS"),
    (TheKinBoss, "THE_KIN_BOSS"),
    (CorpseSlugsWeak, "CORPSE_SLUGS_WEAK"),
    (SeapunkWeak, "SEAPUNK_WEAK"),
    (SludgeSpinnerWeak, "SLUDGE_SPINNER_WEAK"),
    (ToadpolesWeak, "TOADPOLES_WEAK"),
    (CorpseSlugsNormal, "CORPSE_SLUGS_NORMAL"),
    (CultistsNormal, "CULTISTS_NORMAL"),
    (FossilStalkerNormal, "FOSSIL_STALKER_NORMAL"),
    (GremlinMercNormal, "GREMLIN_MERC_NORMAL"),
    (HauntedShipNormal, "HAUNTED_SHIP_NORMAL"),
    (LivingFogNormal, "LIVING_FOG_NORMAL"),
    (PunchConstructNormal, "PUNCH_CONSTRUCT_NORMAL"),
    (PunchOffEventEncounter, "PUNCH_OFF_EVENT_ENCOUNTER"),
    (SeapunkNormal, "SEAPUNK_NORMAL"),
    (SewerClamNormal, "SEWER_CLAM_NORMAL"),
    (ToadpolesNormal, "TOADPOLES_NORMAL"),
    (TwoTailedRatsNormal, "TWO_TAILED_RATS_NORMAL"),
    (PhantasmalGardenersElite, "PHANTASMAL_GARDENERS_ELITE"),
    (SkulkingColonyElite, "SKULKING_COLONY_ELITE"),
    (TerrorEelElite, "TERROR_EEL_ELITE"),
    (WaterfallGiantBoss, "WATERFALL_GIANT_BOSS"),
    (SoulFyshBoss, "SOUL_FYSH_BOSS"),
    (LagavulinMatriarchBoss, "LAGAVULIN_MATRIARCH_BOSS"),
    (FakeMerchantEventEncounter, "FAKE_MERCHANT_EVENT_ENCOUNTER"),
    (DenseVegetationEvent, "DENSE_VEGETATION_EVENT"),
    (ThievingHopperWeak, "THIEVING_HOPPER_WEAK"),
    (TunnelerWeak, "TUNNELER_WEAK"),
    (BowlbugsWeak, "BOWLBUGS_WEAK"),
    (ExoskeletonsWeak, "EXOSKELETONS_WEAK"),
    (BowlbugsNormal, "BOWLBUGS_NORMAL"),
    (ChompersNormal, "CHOMPERS_NORMAL"),
    (ExoskeletonsNormal, "EXOSKELETONS_NORMAL"),
    (HunterKillerNormal, "HUNTER_KILLER_NORMAL"),
    (LouseProgenitorNormal, "LOUSE_PROGENITOR_NORMAL"),
    (MytesNormal, "MYTES_NORMAL"),
    (OvicopterNormal, "OVICOPTER_NORMAL"),
    (SlumberingBeetleNormal, "SLUMBERING_BEETLE_NORMAL"),
    (SpinyToadNormal, "SPINY_TOAD_NORMAL"),
    (TheObscuraNormal, "THE_OBSCURA_NORMAL"),
    (TunnelerNormal, "TUNNELER_NORMAL"),
    (DecimillipedeElite, "DECIMILLIPEDE_ELITE"),
    (EntomancerElite, "ENTOMANCER_ELITE"),
    (InfestedPrismsElite, "INFESTED_PRISMS_ELITE"),
    (TheInsatiableBoss, "THE_INSATIABLE_BOSS"),
    (KnowledgeDemonBoss, "KNOWLEDGE_DEMON_BOSS"),
    (KaiserCrabBoss, "KAISER_CRAB_BOSS"),
    (DevotedSculptorWeak, "DEVOTED_SCULPTOR_WEAK"),
    (ScrollsOfBitingWeak, "SCROLLS_OF_BITING_WEAK"),
    (TurretOperatorWeak, "TURRET_OPERATOR_WEAK"),
    (AxebotsNormal, "AXEBOTS_NORMAL"),
    (ConstructMenagerieNormal, "CONSTRUCT_MENAGERIE_NORMAL"),
    (FabricatorNormal, "FABRICATOR_NORMAL"),
    (FrogKnightNormal, "FROG_KNIGHT_NORMAL"),
    (GlobeHeadNormal, "GLOBE_HEAD_NORMAL"),
    (OwlMagistrateNormal, "OWL_MAGISTRATE_NORMAL"),
    (ScrollsOfBitingNormal, "SCROLLS_OF_BITING_NORMAL"),
    (SlimedBerserkerNormal, "SLIMED_BERSERKER_NORMAL"),
    (TheLostAndForgottenNormal, "THE_LOST_AND_FORGOTTEN_NORMAL"),
    (KnightsElite, "KNIGHTS_ELITE"),
    (MechaKnightElite, "MECHA_KNIGHT_ELITE"),
    (SoulNexusElite, "SOUL_NEXUS_ELITE"),
    (MysteriousKnightEventEncounter, "MYSTERIOUS_KNIGHT_EVENT_ENCOUNTER"),
    (AeonglassBoss, "AEONGLASS_BOSS"),
    (DoormakerBoss, "DOORMAKER_BOSS"),
    (QueenBoss, "QUEEN_BOSS"),
    (TestSubjectBoss, "TEST_SUBJECT_BOSS"),
    (BattlewornDummyEventV1Encounter, "BATTLEWORN_DUMMY_EVENT_V1_ENCOUNTER"),
    (BattlewornDummyEventV2Encounter, "BATTLEWORN_DUMMY_EVENT_V2_ENCOUNTER"),
    (BattlewornDummyEventV3Encounter, "BATTLEWORN_DUMMY_EVENT_V3_ENCOUNTER"),
}
