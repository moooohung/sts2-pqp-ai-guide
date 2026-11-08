//! Owner-indexed relic plumbing. Admission is deliberately opt-in per audited relic.
use crate::{card::{Card, CId}, combat::{Combat, PLAYER}, tables::*};

pub const OWNER_AWARE_RELICS: &[RelicId] = &[
    RelicId::BurningBlood, RelicId::HistoryCourse, RelicId::ChoicesParadox,
    RelicId::Toolbox, RelicId::GamblingChip, RelicId::ToastyMittens,
    RelicId::BloodVial, RelicId::UnceasingTop,
    // Audited Models/Relics/{Lantern,HappyFlower,Candelabra,Chandelier}.cs:
    // AfterSideTurnStart targets Owner and uses Owner.PlayerCombatState.TurnNumber.
    // VenerableTeaSet.AfterEnergyReset guards player == Owner.
    // SparklingRouge.AfterBlockCleared guards creature == Owner.Creature.
    // ArtOfWar.AfterCardPlayed/AfterSideTurnEnd/AfterEnergyReset track Owner's turns.
    // OrnamentalFan/LetterOpener.AfterCardPlayed count only Owner's cards in progress.
    // Pendulum.BeforeHandDraw/ModifyHandDraw both guard player == Owner.
    RelicId::Lantern, RelicId::LetterOpener, RelicId::HappyFlower,
    RelicId::VenerableTeaSet, RelicId::Chandelier, RelicId::Candelabra,
    RelicId::SparklingRouge, RelicId::ArtOfWar, RelicId::OrnamentalFan,
    RelicId::Pendulum,
    // Batch 2: audited Models/Relics/<Name>.cs. Each body and modifier below
    // uses Owner.Creature/Owner.PlayerCombatState or guards player/card.Owner.
    // LastingCandy has reward hooks only, not an AfterCombatEnd callback.
    RelicId::Akabeko, RelicId::MiniatureCannon, RelicId::IceCream,
    RelicId::LastingCandy, RelicId::CloakClasp, RelicId::Nunchaku,
    RelicId::Pocketwatch, RelicId::GamePiece, RelicId::HornCleat,
    RelicId::MercuryHourglass,
    // Census group 1: each model audited against Models/Relics/<Name>.cs.
    RelicId::Anchor, RelicId::Kusarigama, RelicId::Permafrost,
    RelicId::StoneCracker, RelicId::Gorget,
    // Census group 2: audited Models/Relics/<Name>.cs callbacks and modifiers.
    RelicId::Girya, RelicId::Shuriken, RelicId::UnsettlingLamp,
    RelicId::BronzeScales, RelicId::OddlySmoothStone,
    // Census group 3: audited Models/Relics/<Name>.cs, including card-source
    // ownership for Vambrace and owner inventory for PetrifiedToad.
    RelicId::RainbowRing, RelicId::Vajra, RelicId::PetrifiedToad,
    RelicId::RedMask, RelicId::Vambrace,
    // Census group 4: Models/Relics/<Name>.cs owner/participant guards.
    RelicId::Orichalcum, RelicId::BeatingRemnant, RelicId::BagOfMarbles,
    RelicId::Kunai, RelicId::MeatOnTheBone,
    // Next census group 1: audited Models/Relics/<Name>.cs.
    RelicId::StrikeDummy, RelicId::ParryingShield, RelicId::TuningFork,
    RelicId::PenNib, RelicId::CaptainsWheel,
    // Next census group 2: Models/Relics/<Name>.cs.
    RelicId::JossPaper, RelicId::CentennialPuzzle, RelicId::Bellows,
    RelicId::MummifiedHand, RelicId::SturdyClamp,
    // BeltBuckle.cs / ReptileTrinket.cs: lifecycle listeners use their own
    // inventory; only ReptileTrinket matches the potion event's owner.
    RelicId::BeltBuckle, RelicId::ReptileTrinket,
    // Census group 7: Models/Relics/<Name>.cs; death and post-Osty HP hooks
    // reuse existing dispatch, without changing shared damage.rs death handling.
    RelicId::GremlinHorn, RelicId::BagOfPreparation, RelicId::StoneCalendar,
    RelicId::TungstenRod, RelicId::RazorTooth,
    // LizardTail.ShouldDieLate/AfterPreventingDeath: dying Owner only.
    RelicId::LizardTail,
    // Models/Relics/{VexingPuzzlebox,RippleBasin,IntimidatingHelmet,RuinedHelmet}.cs.
    RelicId::VexingPuzzlebox, RelicId::RippleBasin,
    RelicId::IntimidatingHelmet, RelicId::RuinedHelmet,
    // Census group 9, Models/Relics/<Name>.cs. PaperPhrog is invoked only by
    // VulnerablePower through the dealer's own player inventory (hooks.rs).
    RelicId::FestivePopper, RelicId::CharonsAshes, RelicId::RedSkull,
    RelicId::SelfFormingClay, RelicId::PaperPhrog,
    // DemonTongue.cs AfterDamageReceived/BeforeSideTurnStart.
    RelicId::DemonTongue,
    // Wide A-M batch 1: Models/Relics/<Name>.cs owner-scoped callbacks.
    RelicId::DelicateFrond, RelicId::BurningSticks, RelicId::Byrdpip,
    RelicId::DivineRight, RelicId::EmberTea,
    // Wide A-M batch 2; FakeAnchor.BeforeCombatStart already uses owner block.
    RelicId::FakeSneckoEye, RelicId::FakeAnchor, RelicId::GhostSeed,
    RelicId::LavaLamp, RelicId::MusicBox,
    // Wide A-M batch 3: audited Models/Relics/<Name>.cs owner callbacks/modifiers.
    RelicId::InfusedCore, RelicId::FurCoat, RelicId::BrilliantScarf,
    RelicId::BigHat, RelicId::FencingManual, RelicId::Bookmark,
    RelicId::DataDisk, RelicId::GoldPlatedCables,
    // N-Z batch b1: audited Models/Relics/<Name>.cs owner-scoped callbacks.
    RelicId::SlingOfCourage, RelicId::SwordOfJade, RelicId::TeaOfDiscourtesy,
    RelicId::PhilosophersStone, RelicId::ThrowingAxe,
    // N-Z batch b2: Models/Relics/<Name>.cs owner-scoped callbacks.
    RelicId::PhylacteryUnbound, RelicId::OrangeDough, RelicId::RunicCapacitor,
    RelicId::PaelsLegion, RelicId::SneckoEye,
    // N-Z batch b3: Models/Relics/<Name>.cs owner-scoped listeners/modifiers.
    RelicId::PollinousCore, RelicId::VitruvianMinion, RelicId::VelvetChoker,
    RelicId::TwistedFunnel, RelicId::UndyingSigil, RelicId::SymbioticVirus,
    RelicId::Tingsha, RelicId::ToughBandages,
    // N-Z batch b4: audited Models/Relics/<Name>.cs.
    RelicId::RingOfTheSnake, RelicId::NinjaScroll, RelicId::SneckoSkull,
    RelicId::PaperKrane, RelicId::Regalite, RelicId::PowerCell,
    RelicId::NutritiousOyster, RelicId::Sai,
    // N-Z batch b5: six acquisition-only models plus owner draw/autoplay.
    RelicId::PrecariousShears, RelicId::PaelsGrowth, RelicId::Whetstone,
    RelicId::PandorasBox, RelicId::NeowsTalisman, RelicId::PotionBelt,
    RelicId::RingOfTheDrake, RelicId::WhisperingEarring,
    // N-Z batch b6: Models/Relics/<Name>.cs; acquisition/rest hooks never
    // execute in combat. Energy and cost callbacks guard the owner.
    RelicId::PrismaticGem, RelicId::Pantograph, RelicId::Shovel,
    RelicId::SpikedGauntlets, RelicId::VeryHotCocoa, RelicId::SignetRing,
    RelicId::WongosMysteryTicket, RelicId::SeaGlass,
    // N-Z batch b7: Models/Relics/<Name>.cs owner deck/shuffle/turn/pickup audit.
    RelicId::RegalPillow, RelicId::WingCharm, RelicId::PaelsClaw,
    RelicId::ToyBox, RelicId::TheAbacus, RelicId::RingingTriangle,
    RelicId::WarHammer, RelicId::TriBoomerang,
    // N-Z batch b8: Models/Relics/<Name>.cs; only TheBoot acts in combat.
    RelicId::WongoCustomerAppreciationBadge, RelicId::TouchOfOrobas,
    RelicId::NewLeaf, RelicId::WhiteBeastStatue, RelicId::TheCourier,
    RelicId::SilverCrucible, RelicId::SmallCapsule, RelicId::TheBoot,
    // N-Z batch b9: pickup/deck-only models, and PumpkinCandle's owner energy.
    RelicId::NeowsSacrifice, RelicId::Strawberry, RelicId::PumpkinCandle,
    RelicId::PunchDagger, RelicId::ToxicEgg, RelicId::SereTalon,
    RelicId::TanxsWhistle, RelicId::NeowsTorment,
    // N-R batch b10: Models/Relics/PaelsTears.cs:38,48,57;
    // PaelsBlood.cs:14 / RunicPyramid.cs:10 guard player == Owner.
    // Other models have acquisition/reward/room hooks only (hooks.rs).
    RelicId::PaelsTears, RelicId::PaelsBlood, RelicId::PhialHolster,
    RelicId::RoyalStamp, RelicId::PaelsWing, RelicId::Planisphere,
    RelicId::RunicPyramid, RelicId::OldCoin,
    // N-R b11, census rank: Models/Relics/<Name>.cs. Six models affect
    // acquisition/rewards only; PaelsFlesh/RoyalPoison use Owner.TurnNumber.
    RelicId::PrayerWheel, RelicId::Orrery, RelicId::Pear, RelicId::PreciseScissors,
    RelicId::PaelsFlesh, RelicId::PaelsHorn, RelicId::PreservedFog, RelicId::RoyalPoison,
    // N-R b12: Models/Relics/Pomander.cs:17 / NutritiousSoup.cs:21 /
    // NeowsBones.cs:38 only acquire deck/reward effects. RadiantPearl.cs:32
    // creates Luminesce only for the starting Owner.
    RelicId::Pomander, RelicId::NutritiousSoup, RelicId::NeowsBones, RelicId::RadiantPearl,
    // Models/Relics/PaelsTooth.cs:93: living Owner, owner's stored cards and rewards RNG.
    RelicId::PaelsTooth,
    RelicId::PaelsEye,
    // H-M batch h1: Models/Relics/<Name>.cs. Four run-layer-only relics
    // have no combat callbacks; MawBank has a pre-room snapshot adapter.
    RelicId::Metronome, RelicId::LunarPastry, RelicId::HelicalDart,
    RelicId::IronClub, RelicId::HeftyTablet, RelicId::LoomingFruit,
    RelicId::MealTicket, RelicId::MawBank,
    // Wide A-M batch 4: Models/Relics/<Name>.cs owner and participant guards.
    RelicId::FakeOrichalcum, RelicId::FuneraryMask, RelicId::GalacticDust,
    RelicId::BoundPhylactery, RelicId::IvoryTile, RelicId::MiniRegent,
    RelicId::BoneFlute, RelicId::CrackedCore,
    // H-M h2: Kifuda/LeadPaperweight/LeesWaffle.AfterObtained only;
    // MoltenEgg modifies rewards, shop results and permanent-deck additions.
    // None has a combat callback, owner turn counter, combat RNG or saved state.
    // Acquisition results belong to the supplied per-owner entry snapshot.
    RelicId::Kifuda, RelicId::LeadPaperweight, RelicId::MoltenEgg, RelicId::LeesWaffle,
    // H-M h3, Models/Relics/<Name>.cs. Acquisition/reward/map/rest
    // callbacks are not combat effects; pickup paths remain owner-fail-closed.
    RelicId::HandDrill, RelicId::MiniatureTent, RelicId::JewelryBox,
    RelicId::MysticLighter, RelicId::LostCoffer, RelicId::Kaleidoscope,
    RelicId::JuzuBracelet, RelicId::LavaRock,
    // A-G batch 5: DreamCatcher / DeprecatedRelic have no combat callbacks;
    // BloodSoakedRose's equipped modifier already guards the queried Owner.
    RelicId::EmotionChip, RelicId::DreamCatcher, RelicId::Crossbow,
    RelicId::DeprecatedRelic, RelicId::FakeHappyFlower, RelicId::BloodSoakedRose,
    RelicId::DragonFruit, RelicId::FishingRod,
    // A-G equipped batch 6, Models/Relics/<Name>.cs: BlackBlood heals Owner.
    // Others affect acquisition/map/deck/rewards only. Combat snapshot loaders
    // must preserve their already-applied effects; do not synthesize pickups.
    RelicId::ArchaicTooth, RelicId::FresnelLens, RelicId::EmptyCage,
    RelicId::GoldenCompass, RelicId::DarkstonePeriapt, RelicId::BlackBlood,
    RelicId::DistinguishedCape, RelicId::FakeMango,
    // H-M h4, Models/Relics/<Name>.cs. Only MrStruggles has a combat
    // callback; the other seven act on shop/rest/deck/acquisition state.
    RelicId::MembershipCard, RelicId::MrStruggles, RelicId::LargeCapsule,
    RelicId::LordsParasol, RelicId::MeatCleaver, RelicId::Mango,
    RelicId::LuckyFysh, RelicId::MassiveScroll,
    // A-G batch 7, Models/Relics/<Name>.cs: GnarledHammer/BeautifulBracelet/
    // Claws are pickup-only; FakeMerchantsRug has no overrides. Preserve the
    // supplied deck/enchantments; never synthesize acquisition in combat.
    RelicId::GnarledHammer, RelicId::DaughterOfTheWind,
    RelicId::FakeMerchantsRug, RelicId::FakeStrikeDummy,
    RelicId::BeautifulBracelet, RelicId::BigMushroom, RelicId::Claws,
    RelicId::FakeBloodVial,
    // H-M h5: LostWisp.AfterCardPlayed and JeweledMask.BeforeHandDraw
    // use Owner; LeafyPoultice.AfterObtained has no combat callback.
    RelicId::LostWisp, RelicId::JeweledMask, RelicId::LeafyPoultice,
    // A-G batch 8, Models/Relics/<Name>.cs. BlessedAntler and BowlerHat have
    // owner-scoped combat paths; the other six affect pickup/deck/rewards only.
    // Snapshot admission never synthesizes those run-layer lifecycle effects.
    RelicId::CallingBell, RelicId::FragrantMushroom, RelicId::BlessedAntler,
    RelicId::BowlerHat, RelicId::Circlet, RelicId::BingBong,
    RelicId::BookOfFiveRings, RelicId::AmethystAubergine,
    // MP run acquisition applies these AfterObtained effects before entry.
    // CursedPearl, GoldenPearl and StoneHumidifier are also admitted below.
    RelicId::LeadPaperweight, RelicId::NeowsTorment, RelicId::SmallCapsule,
    // A-G batch 9: Models/Relics/<Name>.cs. Six affect only acquisition/rewards;
    // equipped snapshots preserve those effects without replaying pickup.
    RelicId::ArcaneScroll, RelicId::DowsingRod, RelicId::AlchemicalCoffer,
    RelicId::DollysMirror, RelicId::Cauldron, RelicId::DivineDestiny,
    RelicId::BlackStar, RelicId::BoomingConch,
    // A-G batch 10, Models/Relics/<Name>.cs: GlassEye/Glitter/Driftwood and
    // EternalFeather have acquisition/reward/rest hooks only. Supplied equipped
    // snapshots preserve those run effects; the other four use Owner guards.
    RelicId::GlassEye, RelicId::Glitter, RelicId::Driftwood, RelicId::Ectoplasm,
    RelicId::FakeVenerableTeaSet, RelicId::Fiddle, RelicId::ChemicalX,
    RelicId::EternalFeather,
    // D-G batch 11, Models/Relics/<Name>.cs. DiamondDiadem/ForgottenSoul use
    // Owner combat callbacks; six other models affect acquisition/deck/rewards.
    RelicId::FakeLeesWaffle, RelicId::DiamondDiadem, RelicId::ForgottenSoul,
    RelicId::FrozenEgg, RelicId::ElectricShrymp, RelicId::DingyRug,
    RelicId::DustyTome, RelicId::GoldenPearl,
    // A-C c1: audited Models/Relics/<Name>.cs; pickup-only models
    // preserve supplied entry state and reject legacy teammate acquisition.
    RelicId::BoneTea, RelicId::Astrolabe, RelicId::Bread, RelicId::ChosenCheese,
    RelicId::BiiigHug, RelicId::CursedPearl, RelicId::BookRepairKnife, RelicId::Brimstone,
    // S-Z s1, Models/Relics/<Name>.cs: TinyMailbox:14-23 and
    // StoneHumidifier:18-24 guard Owner's rest heal. SilkenTress:47-82 is a
    // one-use owner reward modifier. ScrollBoxes:32-43, WarPaint:20-28 and
    // YummyCookie:69-77 are pickup-only; supplied snapshots already include them.
    // SealOfGold:25-32 spends Owner.Gold; SwordOfStone:40-50 counts elite wins.
    RelicId::TinyMailbox, RelicId::SealOfGold, RelicId::SilkenTress,
    RelicId::ScrollBoxes, RelicId::WarPaint, RelicId::YummyCookie,
    RelicId::SwordOfStone, RelicId::StoneHumidifier,
    // S-Z s2, Models/Relics/<Name>.cs: Sozu:17-29 guards the queried Owner;
    // WingedBoots:31-100 affects travel only; WhiteStar:19-30 affects Owner's
    // elite rewards only. SandCastle:22-31 / Storybook:17-21 are pickup-only.
    // ScreamingFlagon:21-27 checks the participating Owner's empty hand.
    RelicId::Sozu, RelicId::WingedBoots, RelicId::ScreamingFlagon,
    RelicId::WhiteStar, RelicId::SandCastle, RelicId::Storybook,
];

impl Combat {
    /// Inline power/relic interactions must use the same admission gate as hooks.
    pub(crate) fn relic_owner_has_enabled(&mut self, owner: CId, id: RelicId) -> bool {
        let mut fault = None;
        let enabled = self.query_relic_owner_has_enabled(owner, id, &mut fault);
        self.apply_query_fault(fault);
        enabled
    }

    pub(crate) fn query_relic_owner_has_enabled(&self, owner: CId, id: RelicId,
        fault: &mut Option<&'static str>) -> bool {
        for r in 0..self.run.relics.len() {
            if self.run.relics[r].owner == owner && self.run.relics[r].id == id
                && self.query_relic_enabled(r, fault) {
                return true;
            }
        }
        false
    }

    pub(crate) fn relic_owner_deck(&self, owner: CId) -> &[Card] {
        if owner == PLAYER { &self.run.deck } else { &self.players[owner].persistent_deck }
    }

    pub(crate) fn relic_owner_deck_mut(&mut self, owner: CId) -> &mut Vec<Card> {
        if owner == PLAYER { &mut self.run.deck } else { &mut self.players[owner].persistent_deck }
    }

    pub(crate) fn with_relic_owner_deck<T>(&mut self, owner: CId, f: impl FnOnce(&mut crate::combat::RunCtx) -> T) -> T {
        // Reuse the existing upgrade/identity bookkeeping, without copying it or
        // changing the shared instance-id/cache/RNG sequence. Restore both decks.
        if owner != PLAYER {
            std::mem::swap(&mut self.run.deck, &mut self.players[owner].persistent_deck);
        }
        let result = f(&mut self.run);
        if owner != PLAYER {
            std::mem::swap(&mut self.run.deck, &mut self.players[owner].persistent_deck);
        }
        result
    }

    pub(crate) fn relic_owner_deck_card_upgradable(&mut self, owner: CId, i: usize) -> bool {
        self.with_relic_owner_deck(owner, |run| run.is_deck_card_upgradable(i))
    }

    pub(crate) fn relic_owner_upgrade_deck_card(&mut self, owner: CId, i: usize) {
        self.with_relic_owner_deck(owner, |run| run.upgrade_deck_card(i));
    }

    pub(crate) fn relic_owner_gold(&mut self, owner: CId) -> Option<i32> {
        if owner == PLAYER { Some(self.run.gold) }
        else if let Some(gold) = self.players[owner].gold { Some(gold) }
        else { self.unported("relic owner initial gold not supplied"); None }
    }

    pub(crate) fn relic_owner_potions(&mut self, owner: CId) -> Option<&mut Vec<Option<PotionId>>> {
        if owner == PLAYER { Some(&mut self.run.potions) }
        else if self.players[owner].potions.is_some() { self.players[owner].potions.as_mut() }
        else { self.unported("relic owner potion inventory not supplied"); None }
    }
}
