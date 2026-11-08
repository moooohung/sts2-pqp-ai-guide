//! r7 `relics/*` RelicInstance state. Hook behaviour lives in `relics.rs`;
//! a relic id outside `PORTED_RELICS` makes the combat fail closed instead of guessing.

use crate::card::KRef;
use crate::tables::RelicId;

/// Retained CardModel references in HashSet enumeration order (including
/// .NET's LIFO reuse of removed entry slots). Cards survive Deck removal.
#[derive(Clone, Debug, Default)]
pub struct RetainedCardSet {
    pub entries: Vec<Option<crate::card::Card>>,
    free: Vec<usize>,
}

/// The single removal boundary for persistent cards. Refresh retained object
/// values after lifecycle mutations and before detaching cards from their Deck.
pub(crate) fn retain_deck_objects(deck:&mut Vec<crate::card::Card>,relics:&mut [Relic],
    predicate:impl FnMut(&crate::card::Card)->bool) {
    for relic in relics {
        if let Some(set)=&mut relic.bing_bong_cards_to_skip {
            for retained in set.entries.iter_mut().flatten() {
                if let Some(live)=deck.iter().find(|card|card.instance_id==retained.instance_id) {
                    *retained=live.clone();
                }
            }
        }
    }
    deck.retain(predicate);
}

pub(crate) fn remove_deck_object(deck:&mut Vec<crate::card::Card>,relics:&mut [Relic],index:usize)->crate::card::Card {
    let removed=deck[index].clone();
    retain_deck_objects(deck,relics,|card|card.instance_id!=removed.instance_id);
    removed
}
impl RetainedCardSet {
    pub fn remove(&mut self,id:u32)->bool {
        let Some(index)=self.entries.iter().position(|c|c.as_ref().is_some_and(|c|c.instance_id==id)) else{return false;};
        self.entries[index]=None;self.free.push(index);true
    }
    pub fn insert(&mut self,card:crate::card::Card) {
        if self.entries.iter().flatten().any(|c|c.instance_id==card.instance_id) {return;}
        if let Some(index)=self.free.pop(){self.entries[index]=Some(card);}
        else {self.entries.push(Some(card));}
    }
}

#[derive(Clone)]
pub struct Relic {
    pub id: RelicId,
    /// Player owning this relic. Run-layer constructors currently create player-zero relics.
    pub owner: crate::card::CId,
    /// RelicModel.FloorAddedToDeck (0 before acquisition).
    pub floor_added_to_deck: i32,
    pub enabled: bool,
    /// Generic per-relic counter (FishingRod `combats_seen`, ...).
    pub counter: i32,
    pub counter2: i32,
    pub counter3: i32,
    pub flag1: bool,
    pub flag2: bool,
    pub card: Option<KRef>,
    /// UnsettlingLamp._doubledPowers: original instance references, in order.
    /// None is the C# not-yet-initialized list; removed powers remain in the arena.
    pub doubled_powers: Option<Vec<crate::creature::PRef>>,
    /// ArchaicTooth's two immutable saved-card values, in Starter/Ancient order.
    /// These are snapshots, not lookups into the subsequently transformed deck.
    pub saved_cards: Option<Box<[crate::card::Card; 2]>>,
    /// BingBong._cardsToSkip is null before its first eligible Deck callback.
    pub bing_bong_cards_to_skip: Option<RetainedCardSet>,
    /// DustyTome.cs AncientCard SavedProperty; pickup/hover metadata only.
    pub ancient_card: Option<crate::tables::CardId>,
    /// Toy Box provenance. r7 stores these as dynamic relic-instance attributes.
    pub is_wax: bool,
    pub is_melted: bool,
}

impl core::fmt::Debug for Relic {
    fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result {
        // Keep existing selector-witness identities for relics that have never
        // retained a BingBong card. Non-null state remains fully represented.
        let mut d=f.debug_struct("Relic");
        d.field("id",&self.id).field("owner",&self.owner)
            .field("floor_added_to_deck",&self.floor_added_to_deck).field("enabled",&self.enabled)
            .field("counter",&self.counter).field("counter2",&self.counter2).field("counter3",&self.counter3)
            .field("flag1",&self.flag1).field("flag2",&self.flag2).field("card",&self.card)
            .field("doubled_powers",&self.doubled_powers).field("saved_cards",&self.saved_cards)
            .field("ancient_card",&self.ancient_card).field("is_wax",&self.is_wax).field("is_melted",&self.is_melted);
        if let Some(set)=&self.bing_bong_cards_to_skip {d.field("bing_bong_cards_to_skip",set);}
        d.finish()
    }
}

impl Relic {
    pub fn new(id: RelicId) -> Relic {
        Relic {
            id,
            owner: 0,
            floor_added_to_deck: 0,
            enabled: true,
            counter: if id == RelicId::PumpkinCandle { 5 } else { 0 },
            counter2: 0,
            counter3: 0,
            flag1: false, flag2: false, card: None, ancient_card: None,
            doubled_powers: None, saved_cards: None, bing_bong_cards_to_skip: None,
            is_wax: false, is_melted: false,
        }
    }

    /// Pack Fur Coat's up-to-eight map coordinates into the two generic 32-bit state words.
    /// Zero bytes mean unused; real map coordinates are encoded as `(col << 4 | row) + 1`.
    pub fn set_fur_coat_coords(&mut self, coords: &[(i32, i32)]) -> Result<(), ()> {
        self.counter2 = 0;
        self.counter3 = 0;
        for (i, &(col, row)) in coords.iter().enumerate() {
            if i >= 8 || !(0..=14).contains(&col) || !(0..=14).contains(&row) {
                return Err(());
            }
            let encoded = (((col as u32) << 4) | row as u32) + 1;
            let word = if i < 4 { &mut self.counter2 } else { &mut self.counter3 };
            let shift = (i % 4) * 8;
            *word = ((*word as u32) | (encoded << shift)) as i32;
        }
        Ok(())
    }

    #[inline]
    pub fn fur_coat_has_coord(&self, coord: (i32, i32)) -> bool {
        let wanted = ((((coord.0 as u32) << 4) | coord.1 as u32) + 1) as u8;
        [self.counter2 as u32, self.counter3 as u32]
            .into_iter()
            .any(|word| (0..4).any(|i| ((word >> (i * 8)) as u8) == wanted))
    }
}

/// Relics whose every r7 hook is ported.
pub const PORTED_RELICS: &[RelicId] = &[
    RelicId::WingedBoots, RelicId::WhiteStar, RelicId::SandCastle, RelicId::Storybook,
    RelicId::TinyMailbox, RelicId::SilkenTress, RelicId::ScrollBoxes,
    RelicId::WarPaint, RelicId::YummyCookie, RelicId::StoneHumidifier,
    RelicId::FakeLeesWaffle, RelicId::FrozenEgg, RelicId::ElectricShrymp,
    RelicId::DingyRug, RelicId::DustyTome, RelicId::GoldenPearl,
    RelicId::GlassEye, RelicId::Glitter, RelicId::Driftwood, RelicId::EternalFeather,
    RelicId::ArcaneScroll, RelicId::DowsingRod, RelicId::AlchemicalCoffer,
    RelicId::DollysMirror, RelicId::Cauldron, RelicId::BlackStar,
    RelicId::CallingBell, RelicId::FragrantMushroom, RelicId::BowlerHat,
    RelicId::Circlet, RelicId::BingBong, RelicId::BookOfFiveRings,
    RelicId::AmethystAubergine,
    RelicId::GnarledHammer, RelicId::FakeMerchantsRug, RelicId::FakeStrikeDummy,
    RelicId::BeautifulBracelet, RelicId::Claws,
    // Equipped combat snapshots only; these have no combat callbacks.
    // Acquisition/deck/map/reward effects remain the run layer's responsibility.
    RelicId::ArchaicTooth, RelicId::FresnelLens, RelicId::EmptyCage,
    RelicId::GoldenCompass, RelicId::DarkstonePeriapt,
    RelicId::DistinguishedCape, RelicId::FakeMango,
    RelicId::EmotionChip, RelicId::DreamCatcher, RelicId::DeprecatedRelic, RelicId::DragonFruit,
    RelicId::FuneraryMask, RelicId::GalacticDust, RelicId::BoundPhylactery,
    RelicId::IvoryTile, RelicId::MiniRegent, RelicId::BoneFlute, RelicId::CrackedCore,
    RelicId::InfusedCore, RelicId::BigHat, RelicId::FencingManual,
    RelicId::Bookmark, RelicId::DataDisk, RelicId::GoldPlatedCables,
    RelicId::HistoryCourse,
    RelicId::BurningBlood, RelicId::FishingRod,
    RelicId::BagOfPreparation, RelicId::Lantern,
    RelicId::Anchor, RelicId::BronzeScales, RelicId::RedSkull, RelicId::FestivePopper,
    RelicId::Vajra, RelicId::HornCleat, RelicId::Kusarigama, RelicId::Vambrace,
    RelicId::CentennialPuzzle, RelicId::VeryHotCocoa, RelicId::ReptileTrinket,
    RelicId::Akabeko, RelicId::BloodVial, RelicId::MummifiedHand, RelicId::PaelsFlesh,
    RelicId::RadiantPearl, RelicId::StrikeDummy, RelicId::Bellows, RelicId::BagOfMarbles,
    RelicId::RainbowRing, RelicId::IntimidatingHelmet, RelicId::CloakClasp,
    RelicId::Shuriken, RelicId::RazorTooth, RelicId::CharonsAshes, RelicId::ArtOfWar,
    RelicId::SwordOfStone, RelicId::SwordOfJade, RelicId::BoomingConch,
    RelicId::Gorget, RelicId::OddlySmoothStone, RelicId::Candelabra,
    RelicId::GremlinHorn, RelicId::MiniatureCannon, RelicId::MeatOnTheBone,
    RelicId::RippleBasin, RelicId::OrnamentalFan, RelicId::PaperPhrog,
    RelicId::LetterOpener, RelicId::RedMask, RelicId::RunicPyramid,
    RelicId::Orichalcum, RelicId::ParryingShield, RelicId::Sozu,
    RelicId::DaughterOfTheWind, RelicId::StoneCracker, RelicId::RuinedHelmet,
    RelicId::BiiigHug,
    RelicId::BlackBlood, RelicId::RingOfTheDrake, RelicId::Permafrost,
    RelicId::MercuryHourglass, RelicId::SelfFormingClay, RelicId::SparklingRouge,
    RelicId::CaptainsWheel, RelicId::Chandelier, RelicId::DemonTongue,
    RelicId::GamePiece, RelicId::IceCream, RelicId::Kunai, RelicId::LastingCandy,
    RelicId::Pocketwatch, RelicId::StoneCalendar, RelicId::SturdyClamp,
    RelicId::TungstenRod, RelicId::UnceasingTop, RelicId::MysticLighter,
    RelicId::ScreamingFlagon, RelicId::SlingOfCourage, RelicId::TheAbacus,
    RelicId::BigMushroom, RelicId::BloodSoakedRose, RelicId::DiamondDiadem,
    RelicId::Ectoplasm, RelicId::FakeAnchor, RelicId::FakeBloodVial,
    RelicId::FakeOrichalcum, RelicId::FakeStrikeDummy, RelicId::ForgottenSoul,
    RelicId::LostWisp, RelicId::MrStruggles, RelicId::PaelsBlood,
    RelicId::PrismaticGem, RelicId::RoyalPoison, RelicId::Sai,
    RelicId::SneckoEye, RelicId::TheBoot,
    RelicId::BeatingRemnant, RelicId::Bread, RelicId::Brimstone,
    RelicId::RingingTriangle, RelicId::BlessedAntler, RelicId::FakeSneckoEye,
    RelicId::Fiddle, RelicId::LavaLamp, RelicId::SealOfGold,
    RelicId::SpikedGauntlets, RelicId::VelvetChoker,
    RelicId::BurningSticks, RelicId::JeweledMask, RelicId::DivineDestiny,
    RelicId::DivineRight,
    RelicId::PaelsTears, RelicId::ChosenCheese,
    RelicId::BoneTea, RelicId::BrilliantScarf, RelicId::ChemicalX,
    RelicId::EmberTea, RelicId::FakeHappyFlower,
    RelicId::Girya, RelicId::HandDrill, RelicId::HappyFlower,
    RelicId::IronClub, RelicId::LizardTail, RelicId::MusicBox, RelicId::Nunchaku,
    RelicId::PenNib, RelicId::Pendulum, RelicId::PetrifiedToad,
    RelicId::PollinousCore, RelicId::PumpkinCandle, RelicId::PureWater,
    RelicId::TeaOfDiscourtesy, RelicId::ThrowingAxe, RelicId::TuningFork,
    RelicId::WongosMysteryTicket,
    RelicId::VenerableTeaSet, RelicId::FakeVenerableTeaSet,
    RelicId::JossPaper, RelicId::GamblingChip, RelicId::UnsettlingLamp,
    RelicId::VexingPuzzlebox, RelicId::BeltBuckle, RelicId::GhostSeed,
    RelicId::Toolbox, RelicId::ChoicesParadox, RelicId::Crossbow,
    RelicId::DelicateFrond, RelicId::PhilosophersStone,
    RelicId::ToastyMittens, RelicId::WarHammer,
    RelicId::Byrdpip, RelicId::FurCoat, RelicId::PaelsEye,
    RelicId::PaelsLegion, RelicId::PaelsTooth, RelicId::ToyBox,
    RelicId::WhisperingEarring,
    RelicId::PhylacteryUnbound, RelicId::OrangeDough, RelicId::RunicCapacitor,
    // H-M h1: four combat listeners and four run-layer-only models.
    RelicId::Metronome, RelicId::LunarPastry, RelicId::HelicalDart,
    RelicId::HeftyTablet, RelicId::LoomingFruit, RelicId::MealTicket, RelicId::MawBank,
    // Models/Relics/{Kifuda,LeadPaperweight,MoltenEgg,LeesWaffle}.cs:
    // no in-combat callbacks; never repeat acquisition at combat entry.
    RelicId::Kifuda, RelicId::LeadPaperweight, RelicId::MoltenEgg, RelicId::LeesWaffle,
    RelicId::VitruvianMinion, RelicId::TwistedFunnel, RelicId::UndyingSigil,
    RelicId::SymbioticVirus, RelicId::Tingsha, RelicId::ToughBandages,
    RelicId::RingOfTheSnake, RelicId::NinjaScroll, RelicId::SneckoSkull,
    RelicId::PaperKrane, RelicId::Regalite, RelicId::PowerCell,
    // H-M h3: only HandDrill/MysticLighter have combat effects.
    RelicId::MiniatureTent, RelicId::JewelryBox, RelicId::LostCoffer,
    RelicId::Kaleidoscope, RelicId::JuzuBracelet, RelicId::LavaRock,
    // H-M h4: run-layer-only models; MrStruggles is listed above.
    RelicId::MembershipCard, RelicId::LargeCapsule, RelicId::LordsParasol,
    RelicId::MeatCleaver, RelicId::Mango, RelicId::LuckyFysh, RelicId::MassiveScroll,
    RelicId::LeafyPoultice,
    RelicId::Pantograph,
    RelicId::Astrolabe, RelicId::CursedPearl, RelicId::BookRepairKnife,
];

#[inline]
pub fn is_ported(id: RelicId) -> bool {
    // Relics without any in-combat hook (Neow and run-layer relics) never act in a combat.
    PORTED_RELICS.contains(&id) || crate::tables::RELIC_INFO[id.index()].combat_hooks.is_empty()
}
