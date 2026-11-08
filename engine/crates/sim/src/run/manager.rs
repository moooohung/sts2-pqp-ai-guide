//! Frozen-r7 `run/run_manager.py` foundation.
use crate::character::CardPool;
use crate::character::Character;
use crate::identity::{EncounterId, EventId, EventOptionId};

use arrayvec::ArrayVec;

use crate::card::Card;
use crate::combat::{Combat, REF_CACHE_WORDS, RunCtx, RunRngs};
use crate::relic::Relic;
use crate::rng::{INT_MAX, Rng, deterministic_hash64};
use crate::tables::*;

use super::catalog::*;
use super::events::{
    ChoiceResume, EventDescription, EventEffect, EventOption, EventState, RunCardChoice, RunChoiceCard,
    RunChoiceEffect, SphereItemKind, SphereState,
};
use super::events::native_identity::{EventOptionIdentity, EventOptionSelector};
use super::map::{ActMap, Coord, generate_act_map, generate_spoils_act_map};
use super::rooms::{GeneratedRooms, generate_rooms};
use super::seed_adapter::gym_seed_to_run_seed;
use super::{rest, shop, treasure};

#[path = "relic_nr.rs"]
mod relic_nr;
#[cfg(test)]
#[path = "crystal_sphere_reroll_tests.rs"]
mod crystal_sphere_reroll_tests;
#[cfg(test)]
#[path = "nr_dispatch_tests.rs"]
mod nr_dispatch_tests;

#[cfg(test)]
#[path = "review_tests.rs"]
mod review_tests;
#[cfg(test)]
#[path = "relic_run_owner_review_a_tests.rs"]
mod relic_run_owner_review_a_tests;
#[cfg(test)]
#[path = "relic_deck_owner_a_tests.rs"]
mod relic_deck_owner_a_tests;
#[cfg(test)]
#[path = "relic_review2_run_a_tests.rs"]
mod relic_review2_run_a_tests;
#[cfg(test)]
#[path = "hm_dispatch_review_tests.rs"]
mod hm_dispatch_review_tests;
#[cfg(test)]
#[path = "h4_dispatch_review_tests.rs"]
mod h4_dispatch_review_tests;
#[path = "conveyor.rs"]
pub(super) mod conveyor;
#[path = "future_potions.rs"]
mod future_potions;
#[path = "slippery_bridge.rs"]
pub(super) mod slippery_bridge;
#[cfg(test)]
#[path = "pickup_requester_tests.rs"]
mod pickup_requester_tests;
#[cfg(test)]
#[path = "merchant_potion_tests.rs"]
mod merchant_potion_tests;
#[path = "punch_off.rs"]
mod punch_off;
#[path = "tinker_time.rs"]
mod tinker_time;
#[path = "trial.rs"]
mod trial;
#[cfg(test)]
#[path = "owned_events_tests.rs"]
mod owned_events_tests;

#[cfg(test)]
#[path = "solo_progression_tests.rs"]
mod solo_progression_tests;

const PLAYER_ID: i32 = 1;
const MAX_ACT_VISITS: usize = 32;
#[cfg(test)]
mod template_cache_tests {
    use super::*;

    #[test]
    fn native_reward_ids_include_special_cards_and_removal() {
        let card = Card::from_factory(CardId::StrikeIronclad, false, 4).unwrap();
        let special = RewardItemKind::SpecialCard { owner: crate::combat::PLAYER, card };
        assert_eq!(special.native_kind(), crate::identity::RewardKind::SpecialCard);
        assert_eq!(RewardItemKind::CardRemoval { owner: crate::combat::PLAYER }.native_kind(),
            crate::identity::RewardKind::RemoveCard);
    }

    #[test]
    fn neow_talisman_identity_delta_depends_on_reference_cache() {
        // Frozen r7 cards/base.py::capture_self_mutating_card_progress caches
        // reference factories process-wide. The corpus's preceding combat_entry
        // already lists both keys before seed 4100105 starts (12 -> 14 at Neow).
        // This checks the cause without guessing or changing runtime cache state.
        for warm in [false, true] {
            let mut mgr = RunManager::new(4100105, RunOptions::default());
            let option = mgr.neow.iter().find(|x| x.relic == RelicId::NeowsTalisman)
                .expect("recorded Neow talisman offer").option_id;
            if warm {
                mgr.run_ctx_mut().ref_cache_miss(CardId::StrikeIronclad, false);
                mgr.run_ctx_mut().ref_cache_miss(CardId::DefendIronclad, false);
            }
            let before = mgr.run_ctx().next_card_id;
            mgr.resolve_neow_choice(option).unwrap();
            // Two upgraded instances, plus two reference factories only when cold.
            assert_eq!(mgr.run_ctx().next_card_id - before, if warm { 2 } else { 4 });
        }
    }
}
const MAX_MAP_HISTORY: usize = 96;
const UNKNOWN_BASE: [f64; 4] = [0.10, -1.00, 0.02, 0.03];
#[path = "neow_rewards.rs"]
mod neow_rewards;
#[path = "orobas.rs"]
pub(super) mod orobas;
#[cfg(test)]
#[path = "orobas_position_tests.rs"]
mod orobas_position_tests;
#[path = "pael.rs"]
mod pael;
#[path = "events/tezcatara.rs"]
mod tezcatara;
#[path = "events/darv.rs"]
mod darv;
#[cfg(test)]
#[path = "budget_selector_tests.rs"]
mod budget_selector_tests;
#[path = "events/reflections.rs"]
pub(in crate::run) mod reflections;
#[path = "events/battleworn_dummy.rs"]
pub(in crate::run) mod battleworn_dummy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPhase {
    Event,
    MapChoice,
    Combat,
    CardReward,
    Treasure,
    Shop,
    RestSite,
    RunOver,
}

impl RunPhase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Event => "EVENT",
            Self::MapChoice => "MAP_CHOICE",
            Self::Combat => "COMBAT",
            Self::CardReward => "CARD_REWARD",
            Self::Treasure => "TREASURE",
            Self::Shop => "SHOP",
            Self::RestSite => "REST_SITE",
            Self::RunOver => "RUN_OVER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunActionKind {
    EventChoice,
    Move,
    LeaveShop,
    BuyCard,
    BuyRelic,
    BuyPotion,
    RemoveCard,
    RestOption,
    EndTurn,
    PlayCard,
    UsePotion,
    DiscardPotion,
    Choose,
    ConfirmChoice,
    Skip,
    PickCard,
    PickSpecialCard,
    RerollCardReward,
    SacrificeCardReward,
    PickCardBundle,
    SkipRewards,
    SkipPotion,
    PickPotion,
    SkipRelic,
    PickRelicReward,
    OpenReward,
    Collect,
}

/// Allocation-free action view. Fields not used by a kind stay at their sentinel value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunAction {
    pub kind: RunActionKind,
    /// Wire/replay compatibility spelling. Gameplay dispatch uses typed fields below.
    pub option_id: Option<&'static str>,
    pub event_option_id: Option<EventOptionIdentity>,
    pub rest_option: Option<rest::RestOptionKind>,
    pub label: Option<&'static str>,
    pub description: Option<&'static str>,
    dynamic_description: Option<EventDescription>,
    pub prompt: Option<&'static str>,
    pub enabled: bool,
    pub coord: Option<Coord>,
    pub point_type: Option<MapPointType>,
    pub slot_index: Option<usize>,
    pub target_player_id: Option<i32>,
    pub target_index: Option<usize>,
    pub target_name: Option<&'static str>,
    pub player_id: Option<i32>,
    pub hand_index: Option<usize>,
    pub card_id: Option<CardId>,
    pub cost: Option<i32>,
    pub price: Option<i32>,
    pub card_type_name: Option<&'static str>,
    pub on_sale: Option<bool>,
    pub relic_rarity: Option<RelicRarity>,
    pub potion_rarity: Option<PotionRarity>,
    pub enchantment_0: Option<(&'static str, i32)>,
    pub enchantment_1: Option<(&'static str, i32)>,
    pub index: Option<usize>,
    pub rarity: Option<CardRarity>,
    pub upgraded: Option<bool>,
    pub potion_id: Option<PotionId>,
    pub relic_id: Option<RelicId>,
    pub reward_index: Option<usize>,
    pub reward_type: Option<&'static str>,
    pub selected: Option<bool>,
    pub selected_count: Option<usize>,
    pub source_pile: Option<&'static str>,
    pub transition: Option<&'static str>,
    pub bundle_len: u8,
    pub bundle_card_ids: [Option<CardId>; 3],
    pub bundle_rarities: [Option<CardRarity>; 3],
    pub bundle_upgraded: [bool; 3],
}

impl RunAction {
    /// Boundary adapter for typed native commands and existing replay callers.
    pub fn new(kind: RunActionKind) -> Self {
        Self {
            kind,
            option_id: None,
            event_option_id: None,
            rest_option: None,
            label: None,
            description: None,
            dynamic_description: None,
            prompt: None,
            enabled: true,
            coord: None,
            point_type: None,
            slot_index: None,
            target_player_id: None,
            target_index: None,
            target_name: None,
            player_id: None,
            hand_index: None,
            card_id: None,
            cost: None,
            price: None,
            card_type_name: None,
            on_sale: None,
            relic_rarity: None,
            potion_rarity: None,
            enchantment_0: None,
            enchantment_1: None,
            index: None,
            rarity: None,
            upgraded: None,
            potion_id: None,
            relic_id: None,
            reward_index: None,
            reward_type: None,
            selected: None,
            selected_count: None,
            source_pile: None,
            transition: None,
            bundle_len: 0,
            bundle_card_ids: [None; 3],
            bundle_rarities: [None; 3],
            bundle_upgraded: [false; 3],
        }
    }

    pub fn description_text(&self) -> Option<String> {
        self.dynamic_description
            .map(EventDescription::render)
            .or_else(|| self.description.map(str::to_owned))
    }

    pub fn label_text(&self) -> Option<String> {
        self.label.map(|label| {
            if self.kind == RunActionKind::EventChoice && let Some(price) = self.price {
                format!("{label} ({price}g)")
            } else {
                label.to_owned()
            }
        })
    }
}

#[inline]
fn push_move_action(
    out: &mut Vec<RunAction>,
    coord: Coord,
    point_type: MapPointType,
) {
    let mut action = RunAction::new(RunActionKind::Move);
    action.coord = Some(coord);
    action.point_type = Some(point_type);
    out.push(action);
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    /// Offline proof compatibility only: frozen r7 Kaleidoscope uses combat pools.
    /// C# Kaleidoscope.cs uses noncombat Other + NoCardPoolModifications.
    #[cfg(feature="selector-proof")]
    pub int1_training_reward_pools: bool,
    /// Live UI scheduling for training; use Sequential for installed-selector replays.
    pub choice_scheduling: crate::combat::ChoiceScheduling,
    /// The simulator's full-unlock card-pool mode; partial epochs are not yet ported.
    pub all_cards_unlocked: bool,
    /// Run-start profile input, not this run's earnings. Defaults to a fresh
    /// profile (0); explicitly supplying None still represents unknown progress.
    pub saved_wongo_points: Option<i32>,
    pub ascension: i32,
    pub hp_cheat: Option<i32>,
    pub discovered_underdocks: bool,
    /// Frozen r7 defaults missing unlock state to the first-run count (0).
    pub number_of_runs: i32,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            #[cfg(feature="selector-proof")]
            int1_training_reward_pools: false,
            choice_scheduling: crate::combat::ChoiceScheduling::Live,
            all_cards_unlocked: true,
            saved_wongo_points: Some(0),
            ascension: 10,
            hp_cheat: None,
            discovered_underdocks: true,
            number_of_runs: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MapHistoryEntry {
    map_point_type: MapPointType,
    room_type: RoomType,
}

#[derive(Clone, Debug)]
pub(super) struct RelicBags {
    pub(super) shared: [Vec<RelicId>; 4],
    pub(super) player: Vec<[Vec<RelicId>; 4]>,
}

impl RelicBags {
    fn remove_obtained(&mut self, owner: usize, id: RelicId) -> Result<(), &'static str> {
        // C# RelicCmd.Obtain removes only the owner's bag and the shared bag.
        let player = self.player.get_mut(owner).ok_or("relic bag: missing owner")?;
        if !RELIC_INFO[id.index()].stackable {
            for bag in player.iter_mut().chain(self.shared.iter_mut()) {
                bag.retain(|entry| *entry != id);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct ActRuntime {
    def: &'static ActDef,
    rooms: GeneratedRooms,
    map: Option<ActMap>,
    normal_cursor: usize,
    elite_cursor: usize,
    event_cursor: usize,
}

#[derive(Clone, Debug)]
enum CardRewardRerollOptions {
    Encounter(RoomType),
    Noncombat {
        character: crate::character::CardPool,
        count: usize,
        rarity: Option<CardRarity>,
        roll_upgrade: bool,
        modify_pool: bool,
    },
    Uniform { character: crate::character::CardPool, count: usize, rarity: CardRarity },
    FuturePotions { rarity: CardRarity, kind: CardType },
    CrystalSphere(CardRarity),
    EmptyPool,
}

/// C# CardReward.OnSelect's chosenCardIds and Reroll's _hasBeenRerolled;
/// exposed as r7 CardReward.cards_picked / rerolls_used counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CardRewardProgress { cards_picked: u32, rerolls_used: u32 }

#[derive(Clone, Debug)]
enum RewardItemKind {
    Potion(PotionId),
    Relic(RelicId),
    SpecialCard { owner: crate::card::CId, card: Card },
    CardRemoval { owner: crate::card::CId },
    Card {
        cards: ArrayVec<Card, 4>,
        closed: bool,
        reroll: Option<CardRewardRerollOptions>,
        progress: CardRewardProgress,
    },
    CardBundles {
        bundles: ArrayVec<ArrayVec<Card, 3>, 2>,
    },
}

include!(concat!(env!("OUT_DIR"), "/gen_manager_identity.rs"));

#[derive(Clone, Debug)]
struct RewardItem {
    kind: RewardItemKind,
    consumed: bool,
    skippable: bool,
    skip_completes: bool,
    add_injury_after: bool,
    neows_bones_final: bool,
}

#[derive(Clone, Debug, Default)]
struct RewardState {
    // Original GoldRewards are auto-collected by this coarser native driver.
    // Retain their generated amounts for immutable offline replay inspection.
    replay_gold: Vec<i32>,
    sphere_rng: Option<Rng>,
    // ToyBox.cs AfterObtained offers a homogeneous set of wax RelicRewards.
    wax_relics: bool,
    // Frozen rewards-screen order (Potion=2, Relic=3, Card=5).
    items: Vec<RewardItem>,
    // Index into items; None means the rewards container itself is active.
    active: Option<usize>,
    // OfferCustom potion sets remain button containers between selections.
    stay_in_container: bool,
}

#[derive(Clone, Copy, Debug)]
struct NeowChoice {
    option_id: super::EventOptionIdentity,
    relic: RelicId,
}

pub(super) fn multiplayer_neow_offers(seed: u64) -> [RelicId; 3] {
    RunManager::generate_neow_choices_for_party(seed, true).map(|choice| choice.relic)
}

pub(super) fn multiplayer_neow_event(seed: u64) -> ([RelicId; 3], Rng) {
    let (choices, rng) = RunManager::generate_neow_event_for_party(seed, true);
    (choices.map(|choice| choice.relic), rng)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunDeckChoiceKind {
    Smith { rest_option: usize },
    ShopRemove { gold_spent: i32 },
    Cook { rest_option: usize },
}

#[derive(Clone, Debug)]
struct RunDeckChoice {
    kind: RunDeckChoiceKind,
    candidates: Vec<usize>,
    selected: Vec<usize>,
}

#[cfg(test)]
#[path = "ordered_choice_tests.rs"]
mod ordered_choice_tests;

/// Owned semantic selector snapshot. Indices retain toggle order, not deck order.
/// This contains no encoded rows, vocabulary IDs, masks or routing/sort keys.
#[derive(Clone, Debug)]
pub struct ObservationRunChoice {
    pub source: Option<&'static str>,
    pub prompt: &'static str,
    pub options: Vec<ObservationChoiceCard>,
    pub selected_in_order: Vec<usize>,
    pub min_choices: usize,
    pub max_choices: usize,
    pub allow_skip: bool,
    pub single_select: bool,
    pub is_multi: bool,
    pub can_confirm: bool,
    pub generated_cards: Vec<Card>,
    pub operation: ObservationChoiceOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationChoiceOperation {
    AddCards,
    Upgrade,
    Enchant { name: &'static str, amount: i32 },
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationChoiceCard {
    pub instance_id: u32,
    pub card_id: CardId,
    pub source_pile: &'static str,
}

#[derive(Clone, Debug)]
pub struct RunManager {
    // C# Player.Character.CardPool. This run constructor creates Ironclad;
    // keep ownership explicit rather than deriving it from cross-color cards.
    character: crate::character::Character,
    /// Native constructor identity retained for read-only observation queries.
    native_character: super::mp::Character,
    // C# RunState.Players.Count; persists across combat ownership transfers.
    player_count: usize,
    fixture_seed: u64,
    master_seed: u64,
    options: RunOptions,
    phase: RunPhase,
    /// Distinguish the initial display marker from a refusal after dispatch.
    native_decision_started: bool,
    /// Fail-closed marker. The lane must stop when this is `Some`.
    pub unported: Option<&'static str>,
    run: Option<RunCtx>,
    initial_run: RunCtx,
    combat: Option<Combat>,
    encounter_id: Option<EncounterId>,
    manager_rng: Rng,
    map_rngs: Vec<Rng>,
    acts: Vec<ActRuntime>,
    current_act: usize,
    current_point: u8,
    act_floor: i32,
    visited_points: ArrayVec<u8, MAX_ACT_VISITS>,
    map_history: ArrayVec<MapHistoryEntry, MAX_MAP_HISTORY>,
    unknown_odds: [f64; 4],
    relic_bags: RelicBags,
    card_rarity_offset: f32,
    potion_reward_odds: f32,
    reward: RewardState,
    // Suspended RewardsSet continuations; the bool requests Neow's Bones' final curse.
    reward_parents: Vec<(RewardState, bool)>,
    treasure_relic: Option<RelicId>,
    shop: shop::ShopState,
    shop_removals_used: u32,
    rest: rest::RestState,
    run_choice: Option<RunDeckChoice>,
    reward_returns_to_rest: bool,
    punch_off_combat: bool,
    fake_merchant_loot: Vec<RelicId>,
    parasol_next_relic: Option<usize>,
    lantern_key_reward: Option<Card>,
    awaiting_next_act: bool,
    neow: [NeowChoice; 3],
    // sim_r1_events: current event and run-layer deck modal.
    current_event: Option<EventState>,
    // Read-only replay provenance for events the native driver finishes eagerly.
    replay_event: Option<EventState>,
    pending_run_choice: Option<RunCardChoice>,
    visited_event_ids: ArrayVec<crate::identity::EventId, 64>,
    wongo_points_earned: i32,
}

#[path = "native_view.rs"]
pub mod native_view;
#[path = "merchant_fixture.rs"]
pub mod merchant_fixture;
#[path = "replay_view_solo.rs"]
mod replay_view;

fn relic_ids(names: &[&str]) -> Vec<RelicId> {
    names
        .iter()
        .map(|name| RelicId::from_name(name).unwrap_or_else(|| panic!("unknown relic {name}")))
        .collect()
}

fn make_run_rngs(master_seed: u64) -> RunRngs {
    let seed = deterministic_hash64(&master_seed.to_string());
    make_run_rngs_from_numeric_seed(seed)
}

fn make_run_rngs_from_numeric_seed(seed: u64) -> RunRngs {
    let player_seed = seed;
    RunRngs {
        seed,
        up_front: Rng::named(seed, "up_front"),
        shuffle: Rng::named(seed, "shuffle"),
        unknown_map_point: Rng::named(seed, "unknown_map_point"),
        combat_card_generation: Rng::named(seed, "combat_card_generation"),
        combat_potion_generation: Rng::named(seed, "combat_potion_generation"),
        combat_card_selection: Rng::named(seed, "combat_card_selection"),
        combat_energy_costs: Rng::named(seed, "combat_energy_costs"),
        combat_targets: Rng::named(seed, "combat_targets"),
        monster_ai: Rng::named(seed, "monster_ai"),
        niche: Rng::named(seed, "niche"),
        combat_orbs: Rng::named(seed, "combat_orbs"),
        treasure_room: Rng::named(seed, "treasure_room_relics"),
        rewards: Rng::named(player_seed, "rewards"),
        shops: Rng::named(player_seed, "shops"),
        transformations: Rng::named(player_seed, "transformations"),
    }
}

#[cfg(test)]
#[test]
fn numeric_seed_matches_existing_solo_adapter() {
    use super::mp::Character;
    for seed in [1, 73003, 4_100_000] {
        for character in [Character::Ironclad, Character::Silent, Character::Defect,
            Character::Necrobinder, Character::Regent] {
            for ascension in [0, 10] {
                let options = RunOptions { ascension, ..RunOptions::default() };
                let original = RunManager::new_for_character(seed, options, character);
                let derived = deterministic_hash64(&gym_seed_to_run_seed(seed).to_string());
                let mut numeric = RunManager::from_numeric_seed(derived, options, character);
                // Adapter bookkeeping is not part of the original game's state.
                numeric.fixture_seed = original.fixture_seed;
                numeric.master_seed = original.master_seed;
                numeric.manager_rng = original.manager_rng.clone();
                assert_eq!(format!("{original:?}"), format!("{numeric:?}"));
            }
        }
    }
}

fn neow_label(id: RelicId) -> &'static str {
    match id {
        RelicId::ArcaneScroll => "Arcane Scroll",
        RelicId::BoomingConch => "Booming Conch",
        RelicId::FishingRod => "Fishing Rod",
        RelicId::GoldenPearl => "Golden Pearl",
        RelicId::Kaleidoscope => "Kaleidoscope",
        RelicId::LeadPaperweight => "Lead Paperweight",
        RelicId::LostCoffer => "Lost Coffer",
        RelicId::MassiveScroll => "Massive Scroll",
        RelicId::NeowsTorment => "Neows Torment",
        RelicId::NewLeaf => "New Leaf",
        RelicId::PhialHolster => "Phial Holster",
        RelicId::PreciseScissors => "Precise Scissors",
        RelicId::ScrollBoxes => "Scroll Boxes",
        RelicId::WingedBoots => "Winged Boots",
        RelicId::LavaRock => "Lava Rock",
        RelicId::SmallCapsule => "Small Capsule",
        RelicId::NutritiousOyster => "Nutritious Oyster",
        RelicId::StoneHumidifier => "Stone Humidifier",
        RelicId::NeowsTalisman => "Neows Talisman",
        RelicId::Pomander => "Pomander",
        RelicId::CursedPearl => "Cursed Pearl",
        RelicId::DowsingRod => "Dowsing Rod",
        RelicId::HeftyTablet => "Hefty Tablet",
        RelicId::LargeCapsule => "Large Capsule",
        RelicId::LeafyPoultice => "Leafy Poultice",
        RelicId::NeowsBones => "Neows Bones",
        RelicId::NeowsSacrifice => "Neows Sacrifice",
        RelicId::PrecariousShears => "Precarious Shears",
        RelicId::SilkenTress => "Silken Tress",
        RelicId::SilverCrucible => "Silver Crucible",
        _ => id.name(),
    }
}

fn philosopher_label(pool: crate::character::Character) -> &'static str {
    match pool {
        Character::Necrobinder => "Necrobinder Philosopher",
        Character::Ironclad => "Ironclad Philosopher",
        Character::Regent => "Regent Philosopher",
        Character::Silent => "Silent Philosopher",
        Character::Defect => "Defect Philosopher",
        _ => "Philosopher",
    }
}

fn philosopher_description(pool: crate::character::Character) -> &'static str {
    match pool {
        Character::Necrobinder => "Card rewards from Necrobinder pool",
        Character::Ironclad => "Card rewards from Ironclad pool",
        Character::Regent => "Card rewards from Regent pool",
        Character::Silent => "Card rewards from Silent pool",
        Character::Defect => "Card rewards from Defect pool",
        _ => "Card rewards from selected pool",
    }
}

impl RunManager {
    pub fn new(seed: u64, options: RunOptions) -> Self {
        Self::new_for_character(seed,options,super::mp::Character::Ironclad)
    }

    pub fn new_for_character(seed:u64,options:RunOptions,character:super::mp::Character)->Self {
        let master_seed = gym_seed_to_run_seed(seed);
        let rngs = make_run_rngs(master_seed);
        Self::initialize(seed, master_seed, rngs, options, character)
    }

    /// Original RunRngSet.cs:55-87 derives a numeric seed once from user text,
    /// then constructs named streams directly. This bypasses only the Gym
    /// seed adapter; it does not load checkpoints or repair RNG counters.
    /// Legacy `old` text has distinct shared/player seeds and is not covered.
    pub fn from_numeric_seed(seed:u64,options:RunOptions,character:super::mp::Character)->Self {
        Self::initialize(seed, seed, make_run_rngs_from_numeric_seed(seed), options, character)
    }

    fn initialize(seed:u64,master_seed:u64,rngs:RunRngs,options:RunOptions,character:super::mp::Character)->Self {
        let run_seed = rngs.seed;

        let mut deck = Vec::with_capacity(24);
        let mut next_card_id = 0u32;
        let (starting_hp,strikes,strike,defend,extra,starting)=character.starting();
        for _ in 0..strikes {
            next_card_id += 1;
            deck.push(Card::from_factory(strike, false, next_card_id).unwrap());
        }
        let defends=if character==super::mp::Character::Ironclad {4}else{strikes};
        for _ in 0..defends {
            next_card_id += 1;
            deck.push(Card::from_factory(defend, false, next_card_id).unwrap());
        }
        for &id in extra {
            next_card_id += 1;
            deck.push(Card::from_factory(id, false, next_card_id).unwrap());
        }
        // Player.PopulateStartingDeck assigns provenance before PopulateDeck,
        // identically for every character (not only Silent).
        for card in &mut deck {card.floor_added_to_deck=Some(1);}

        let max_hp = options.hp_cheat.unwrap_or(starting_hp);
        // C# Player.PopulateStartingRelics: starting relic provenance is floor 1.
        let mut starting_relic = Relic::new(starting);
        starting_relic.floor_added_to_deck = 1;
        let mut run = RunCtx {
            choice_scheduling: options.choice_scheduling,
            all_cards_unlocked: Some(options.all_cards_unlocked),
            rngs,
            ascension: options.ascension,
            act_index: 0,
            total_floor: 0,
            coord: None,
            room_type: RoomType::Event,
            gold: 99,
            extra_gold_reward: 0,
            max_hp,
            hp: max_hp,
            max_energy: 3,
            max_potion_slots: 3,
            base_orb_slots: if character==super::mp::Character::Defect {3}else{0},
            deck,
            relics: vec![starting_relic],
            potions: Vec::with_capacity(3),
            next_card_id,
            // C# RunState.CreateCard/AddCard owns cards per run; ModelDb's canonical
            // cache does not allocate process-wide mutable-card identities. Keep
            // this r7 identity-compatibility cache local, never warmed by another run.
            ref_cache: [0; REF_CACHE_WORDS],
            paels_tooth_cards: Vec::new(),
        };

        // r7 initialize_run: shared bag, player bag, ascension, all room sequences.
        let mut shared = [
            relic_ids(SHARED_RELICS_COMMON),
            relic_ids(SHARED_RELICS_UNCOMMON),
            relic_ids(SHARED_RELICS_RARE),
            relic_ids(SHARED_RELICS_SHOP),
        ];
        // run: run_state.py:_populate_shared_relic_grab_bag. Registry insertion order is
        // UNCOMMON, COMMON, RARE, SHOP, EVENT, ANCIENT. The latter two are not treasure pools,
        // but their shuffles are still part of the frozen UpFront RNG schedule.
        for i in [1usize, 0, 2, 3] {
            run.rngs.up_front.shuffle(&mut shared[i]);
        }
        let mut shared_event = relic_ids(&["FRESNEL_LENS"]);
        run.rngs.up_front.shuffle(&mut shared_event);
        let mut shared_ancient_relics = relic_ids(&["LOOMING_FRUIT", "VERY_HOT_COCOA"]);
        run.rngs.up_front.shuffle(&mut shared_ancient_relics);
        let mut player = [
            relic_ids(IRON_RELICS_COMMON),
            relic_ids(IRON_RELICS_UNCOMMON),
            relic_ids(IRON_RELICS_RARE),
            relic_ids(IRON_RELICS_SHOP),
        ];
        if character!=super::mp::Character::Ironclad {
            // Character RelicPool.GenerateAllRelics insertion order; shared pools
            // retain their canonical order before owner-specific append.
            let own=relic_ids(match character {
                super::mp::Character::Silent => &["HELICAL_DART","NINJA_SCROLL","PAPER_KRANE","RING_OF_THE_SNAKE",
                    "SNECKO_SKULL","TINGSHA","TOUGH_BANDAGES","TWISTED_FUNNEL"],
                super::mp::Character::Defect => &["CRACKED_CORE","DATA_DISK","EMOTION_CHIP","GOLD_PLATED_CABLES",
                    "POWER_CELL","METRONOME","RUNIC_CAPACITOR","SYMBIOTIC_VIRUS"],
                super::mp::Character::Regent => &["DIVINE_RIGHT","FENCING_MANUAL","GALACTIC_DUST","LUNAR_PASTRY",
                    "MINI_REGENT","ORANGE_DOUGH","REGALITE","VITRUVIAN_MINION"],
                super::mp::Character::Necrobinder => &["BIG_HAT","BONE_FLUTE","BOOK_REPAIR_KNIFE","BOOKMARK",
                    "BOUND_PHYLACTERY","FUNERARY_MASK","IVORY_TILE","UNDYING_SIGIL"],
                _ => unreachable!(),
            });
            player=[relic_ids(SHARED_RELICS_COMMON),relic_ids(SHARED_RELICS_UNCOMMON),
                relic_ids(SHARED_RELICS_RARE),relic_ids(SHARED_RELICS_SHOP)];
            for (i,rarity) in [RelicRarity::Common,RelicRarity::Uncommon,RelicRarity::Rare,RelicRarity::Shop]
                .into_iter().enumerate() {
                player[i].extend(own.iter().copied().filter(|id|RELIC_INFO[id.index()].rarity==rarity));
            }
        }
        // run: run_state.py:PlayerState.populate_relic_grab_bag; same registry rarity order.
        for i in [1usize, 0, 2, 3] {
            run.rngs.up_front.shuffle(&mut player[i]);
        }

        if options.ascension >= 4 {
            run.max_potion_slots = (run.max_potion_slots - 1).max(0);
        }
        if options.ascension >= 5 {
            // r7 creates the curse first (id 11), then replaces its instance id with
            // new_card_instance_id_after(deck) (id 12).
            run.next_card_id += 1;
            run.next_card_id += 1;
            run.deck
                .push(Card::from_factory(CardId::AscendersBane, false, run.next_card_id).unwrap());
        }
        let initial_run=run.clone();
        let first = if options.discovered_underdocks {
            let mut select = Rng::named(run_seed, "act_selection");
            if select.choice_index(2) == 0 {
                &ACT0
            } else {
                &ACT1
            }
        } else {
            &ACT1
        };
        let act_defs: [&'static ActDef; 3] = [first, &ACT2, &ACT3];

        let mut shared_ancients = SHARED_ANCIENTS.to_vec();
        run.rngs.up_front.shuffle(&mut shared_ancients);
        let mut shared_by_act: [Vec<EventId>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for slot in shared_by_act.iter_mut().skip(1) {
            let count = run
                .rngs
                .up_front
                .next_int_exclusive(0, shared_ancients.len() as i64 + 1)
                as usize;
            slot.extend(shared_ancients.drain(..count));
        }
        let mut acts = Vec::with_capacity(3);
        for (i, def) in act_defs.into_iter().enumerate() {
            let rooms = generate_rooms(
                def,
                &mut run.rngs.up_front,
                &shared_by_act[i],
                options.ascension >= 10 && i == 2,
            );
            acts.push(ActRuntime {
                def,
                rooms,
                map: None,
                normal_cursor: 0,
                elite_cursor: 0,
                event_cursor: 0,
            });
        }

        let mut map_rngs = (0..3)
            .map(|i| Rng::named(run_seed, &format!("act_{}_map", i + 1)))
            .collect::<Vec<_>>();
        let map = generate_act_map(
            acts[0].def.num_rooms,
            &mut map_rngs[0],
            options.ascension,
            0,
        );
        let start = map.start;
        let start_coord = map.points[start as usize].coord;
        let start_point_type = map.points[start as usize].point_type;
        acts[0].map = Some(map);

        // _enter_neow: visit row 0, then set hp=0 and heal 80% of missing HP at A2+.
        run.total_floor = 1;
        run.coord = Some((start_coord.col, start_coord.row));
        run.room_type = RoomType::Event;
        run.hp = 0;
        let heal = if options.ascension >= 2 {
            (run.max_hp as f64 * 0.8) as i32
        } else {
            run.max_hp
        };
        run.hp = (run.hp + heal).min(run.max_hp);

        let neow = Self::generate_neow_choices(run_seed);
        // r7 `_enter_ancient` advances the regular event cursor once even
        // though the ancient itself comes from the ancient slot. Neow enters
        // through that path at run start.
        acts[0].event_cursor = 1;
        let mut visited_points = ArrayVec::new();
        visited_points.push(start);
        let mut map_history = ArrayVec::new();
        // run: run/run_state.py:1905
        map_history.push(MapHistoryEntry {
            map_point_type: start_point_type,
            room_type: RoomType::Event,
        });
        Self {
            fixture_seed: seed,
            master_seed,
            character: character,
            native_character: character,
            player_count: 1,
            options,
            phase: RunPhase::Event,
            native_decision_started: false,
            unported: Some(if options.all_cards_unlocked { "EVENT:Neow" } else {
                "partial card unlock configuration is not ported"
            }),
            run: Some(run),
            initial_run,
            combat: None,
            encounter_id: None,
            manager_rng: Rng::new(master_seed.wrapping_add(9_999)),
            map_rngs,
            acts,
            current_act: 0,
            current_point: start,
            act_floor: 1,
            visited_points,
            map_history,
            unknown_odds: UNKNOWN_BASE,
            relic_bags: RelicBags { shared, player: vec![player] },
            card_rarity_offset: -0.05,
            potion_reward_odds: 0.40,
            reward: RewardState::default(),
            reward_parents: Vec::new(),
            treasure_relic: None,
            shop: shop::ShopState::default(),
            shop_removals_used: 0,
            rest: rest::RestState::default(),
            run_choice: None,
            reward_returns_to_rest: false,
            punch_off_combat: false,
            fake_merchant_loot: Vec::new(),
            parasol_next_relic: None,
            lantern_key_reward: None,
            awaiting_next_act: false,
            neow,
            current_event: None,
            replay_event: None,
            pending_run_choice: None,
            visited_event_ids: ArrayVec::new(),
            wongo_points_earned: 0,
        }
    }

    fn generate_neow_choices(run_seed: u64) -> [NeowChoice; 3] {
        Self::generate_neow_choices_for_party(run_seed, false)
    }

    fn generate_neow_choices_for_party(run_seed: u64, multiplayer: bool) -> [NeowChoice; 3] {
        Self::generate_neow_event_for_party(run_seed, multiplayer).0
    }

    fn generate_neow_event_for_party(run_seed: u64, multiplayer: bool) -> ([NeowChoice; 3], Rng) {
        let mut rng = Rng::new(run_seed.wrapping_add(deterministic_hash64("NEOW")));
        let cursed_names = [
            "CURSED_PEARL",
            "DOWSING_ROD",
            "HEFTY_TABLET",
            "LARGE_CAPSULE",
            "LEAFY_POULTICE",
            "NEOWS_BONES",
            "NEOWS_SACRIFICE",
            "PRECARIOUS_SHEARS",
            "SILKEN_TRESS",
            "SILVER_CRUCIBLE",
        ];
        // SilverCrucible.IsAllowed:83 restricts it to a one-player run.
        let cursed_names: Vec<_> = cursed_names.into_iter()
            .filter(|name| !multiplayer || *name != "SILVER_CRUCIBLE").collect();
        let cursed =
            RelicId::from_name(cursed_names[rng.choice_index(cursed_names.len())]).unwrap();
        let mut positive = relic_ids(&[
            "ARCANE_SCROLL",
            "BOOMING_CONCH",
            "FISHING_ROD",
            "GOLDEN_PEARL",
            "KALEIDOSCOPE",
            "LEAD_PAPERWEIGHT",
            "LOST_COFFER",
            "MASSIVE_SCROLL",
            "NEOWS_TORMENT",
            "NEW_LEAF",
            "PHIAL_HOLSTER",
            "PRECISE_SCISSORS",
            "SCROLL_BOXES",
            "WINGED_BOOTS",
        ]);
        let remove = |v: &mut Vec<RelicId>, id| v.retain(|x| *x != id);
        match cursed {
            RelicId::CursedPearl => remove(&mut positive, RelicId::GoldenPearl),
            RelicId::HeftyTablet => remove(&mut positive, RelicId::ArcaneScroll),
            RelicId::PrecariousShears => remove(&mut positive, RelicId::PreciseScissors),
            RelicId::LeafyPoultice => remove(&mut positive, RelicId::NewLeaf),
            RelicId::NeowsSacrifice => {
                remove(&mut positive, RelicId::PhialHolster);
                remove(&mut positive, RelicId::LostCoffer);
            }
            _ => {}
        }
        if cursed != RelicId::LargeCapsule {
            positive.push(if rng.next_bool() {
                RelicId::LavaRock
            } else {
                RelicId::SmallCapsule
            });
        }
        positive.push(if rng.next_bool() {
            RelicId::NutritiousOyster
        } else {
            RelicId::StoneHumidifier
        });
        positive.push(if rng.next_bool() {
            RelicId::NeowsTalisman
        } else {
            RelicId::Pomander
        });
        // MassiveScroll.IsAllowed and WingedBoots.IsAllowed have opposite
        // player-count constraints; apply them before the Neow shuffle.
        remove(&mut positive, if multiplayer { RelicId::WingedBoots } else { RelicId::MassiveScroll });
        // ScrollBoxes and Kaleidoscope are available under the exporter profile.
        rng.shuffle(&mut positive);
        ([
            NeowChoice {
                option_id: super::EventOptionIdentity::neow(0),
                relic: positive[0],
            },
            NeowChoice {
                option_id: super::EventOptionIdentity::neow(1),
                relic: positive[1],
            },
            NeowChoice {
                option_id: super::EventOptionIdentity::neow(2),
                relic: cursed,
            },
        ], rng)
    }

    #[inline]
    pub fn phase(&self) -> RunPhase {
        self.phase
    }
    #[inline]
    pub fn fixture_seed(&self) -> u64 {
        self.fixture_seed
    }
    #[inline]
    pub fn master_seed(&self) -> u64 {
        self.master_seed
    }
    #[inline]
    pub fn manager_rng_counter(&self) -> u64 {
        self.manager_rng.counter
    }
    #[inline]
    pub fn map_rng_counter(&self, act: usize) -> Option<u64> {
        self.map_rngs.get(act).map(|r| r.counter)
    }
    #[inline]
    pub fn act_floor(&self) -> i32 {
        self.act_floor
    }
    #[inline]
    pub fn current_act_index(&self) -> usize {
        self.current_act
    }
    #[inline]
    pub fn combat(&self) -> Option<&Combat> {
        self.combat.as_ref()
    }
    pub fn record_replay_combat(&mut self, recorder: &crate::between::Recorder) {
        if let Some(combat) = &mut self.combat { recorder.record_combat(combat); }
    }
    /// Exact room model identity retained through its post-combat rewards.
    pub fn observation_encounter_id(&self) -> Option<&str> {
        self.encounter_id.map(EncounterId::name)
    }
    /// Public current-map boss only; never expose a future act's rooms.
    pub fn public_act1_boss(&self) -> Option<&'static str> {
        (self.current_act == 0 && self.acts[0].map.is_some())
            .then(|| self.acts[0].rooms.boss.name())
    }
    /// Read-only currently visible map; never generates/resolves hidden rooms.
    pub fn visible_map(&self) -> Option<&ActMap> {
        self.acts[self.current_act].map.as_ref()
    }
    /// Lane observation adapter; does not advance the run or bypass its phase.
    /// The caller supplies the exact public boss and choice-prompt context.
    pub fn write_combat_observation(
        &mut self,
        context: &crate::obs::CombatObservationContext<'_>,
        buffers: &mut crate::obs::ObsBuffers<'_>,
        slots: &mut crate::obs::ActionSlots,
    ) -> Result<(), crate::obs::ObsError> {
        if matches!(self.phase, RunPhase::Event | RunPhase::MapChoice) {
            let actions = self.available_actions();
            return crate::obs::write_neow_obs(self, &actions, context, buffers, slots);
        }
        if self.phase != RunPhase::Combat {
            return Err(crate::obs::ObsError::UnsupportedState(self.phase.name()));
        }
        let combat = self.combat.as_mut().ok_or(
            crate::obs::ObsError::UnsupportedState("combat phase without combat"))?;
        crate::obs::write_combat_obs_and_mask(combat, context, buffers, slots)
    }
    #[cfg(feature = "selector-proof")]
    pub fn write_combat_proof_observation(
        &mut self,
        context: &crate::obs::CombatObservationContext<'_>,
        buffers: &mut crate::obs::ObsBuffers<'_>,
        slots: &mut crate::obs::ActionSlots,
    ) -> Result<(), crate::obs::ObsError> {
        if self.phase != RunPhase::Combat {
            return Err(crate::obs::ObsError::UnsupportedState(self.phase.name()));
        }
        let combat = self.combat.as_mut().ok_or(
            crate::obs::ObsError::UnsupportedState("combat phase without combat"))?;
        crate::obs::write_combat_proof_obs_and_mask(combat, context, buffers, slots)
    }
    #[inline]
    pub fn current_event_id(&self) -> Option<&'static str> {
        self.current_event.as_ref().map(|event| event.id.name())
    }

    /// Retained numeric event identity; NativeObsV1 does not tokenize the legacy name.
    pub fn native_event_id(&self) -> Option<crate::identity::EventId> {
        self.current_event.as_ref().map(|event| event.id)
    }

    /// Typed displayed ancient offer; no label parsing or RNG consumption.
    pub fn observation_ancient_relic(&self, option_id: &str) -> Option<RelicId> {
        let event = self.current_event.as_ref()?;
        if !matches!(event.id, EventId::Tezcatara | EventId::Orobas | EventId::Pael | EventId::Vakuu | EventId::Nonupeipe | EventId::DollRoom | EventId::Tanx) { return None; }
        let index = event.options.iter().position(|o| o.option_id.legacy_id() == option_id)?;
        event.relics.get(index).copied()
    }

    /// Ranwid closes callbacks over these exact inventory instances on entry.
    pub fn observation_ranwid_offers(&self) -> Option<(Option<(usize,PotionId)>,Option<(usize,RelicId)>)> {
        let event=self.current_event.as_ref()?;
        if event.id!=EventId::RanwidTheElder || event.ints[2]!=0 {return None;}
        let potion=usize::try_from(event.ints[0]).ok().and_then(|slot|
            self.run_ctx().potions.get(slot).copied().flatten().map(|id|(slot,id)));
        let relic=usize::try_from(event.ints[1]).ok().and_then(|slot|
            self.run_ctx().relics.get(slot).filter(|r|Self::event_relic_is_tradable(r)).map(|r|(slot,r.id)));
        Some((potion,relic))
    }

    pub fn observation_nonupeipe_bracelet_allowed(&self)->bool {
        self.run_ctx().deck.iter().filter(|c|Self::event_can_enchant(c,"Swift")).count()>=4
    }

    pub fn observation_crystal_price(&self)->Option<i32> {
        self.current_event.as_ref().filter(|e|e.id==EventId::CrystalSphere && e.crystal.is_none())
            .map(|e|e.ints[0])
    }

    /// C# ColorfulPhilosophers closes each callback over the selected CardPool.
    /// Read the retained generated pool, never infer it from an ordinal or title.
    pub fn observation_philosopher_pool(&self, ordinal: usize) -> Option<crate::character::Character> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::ColorfulPhilosophers || ordinal >= event.ints[0] as usize { return None; }
        const POOLS: [crate::character::Character; 5] = [crate::character::Character::Necrobinder, crate::character::Character::Ironclad, crate::character::Character::Regent, crate::character::Character::Silent, crate::character::Character::Defect];
        POOLS.get(*event.ints.get(ordinal + 1)? as usize).copied()
    }

    /// Public covered-cell coordinates only; item identities stay private.
    pub fn observation_sphere_hidden_cells(&self) -> Option<Vec<(usize, usize)>> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::CrystalSphere { return None; }
        Some(event.crystal.as_ref()?.hidden_cells())
    }

    pub fn observation_choir_price(&self) -> Option<i32> {
        let event = self.current_event.as_ref()?;
        (event.id == EventId::LuminousChoir).then_some(event.ints[0])
    }

    /// MerchantRelicEntry public model, Cost and IsStocked, in slot order.
    pub fn observation_fake_merchant_inventory(&self) -> Option<Vec<(RelicId, i32, bool)>> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::FakeMerchant { return None; }
        Some(event.relics.iter().enumerate().map(|(i, &id)|
            (id, event.ints[i], event.flags & (1 << i) == 0)).collect())
    }

    /// RelicTrader.GetRelicHoverTips displays both retained models, not labels.
    pub fn observation_relic_trader_offers(&self) -> Option<Vec<(RelicId, RelicId)>> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::RelicTrader { return None; }
        let count = self.run_ctx().relics.iter().filter(|r| Self::event_relic_is_tradable(r)).count().min(3);
        (0..count).map(|i| {
            let owned = self.run_ctx().relics.get(usize::try_from(event.ints[i]).ok()?)?;
            if !Self::event_relic_is_tradable(owned) { return None; }
            Some((owned.id, *event.relics.get(i)?))
        }).collect()
    }

    /// StoneOfAllTime retains the actual potion instance and locks inventory.
    pub fn observation_stone_offers(&self) -> Option<(Option<(usize, PotionId)>, bool)> {
        let event = self.current_event.as_ref()?;
        if event.id != EventId::StoneOfAllTime { return None; }
        let potion = usize::try_from(event.ints[0]).ok().and_then(|slot|
            self.run_ctx().potions.get(slot).copied().flatten().map(|id| (slot, id)));
        let can_push = self.run_ctx().deck.iter().any(|c| Self::event_can_enchant(c, "Vigorous"));
        Some((potion, can_push))
    }
    /// Last entered coordinate; SetActInternal clears it before map travel.
    pub fn observation_map_position(&self) -> Option<(i32, i32)> {
        self.acts.get(self.current_act)?.map.as_ref()?;
        self.run_ctx().coord
    }
    #[inline]
    fn is_multiplayer(&self) -> bool {
        self.player_count > 1
    }

    pub fn run_ctx(&self) -> &RunCtx {
        self.combat
            .as_ref()
            .map_or_else(|| self.run.as_ref().expect("run context"), |c| &c.run)
    }
    /// Read-only selector metadata for the observation encoder.
    pub(crate) fn observation_run_choice(&self) -> Option<RunCardChoice> {
        if let Some(choice)=&self.pending_run_choice { return Some(choice.clone()); }
        let choice=self.run_choice.as_ref()?;
        let options=choice.candidates.iter().filter_map(|&i| self.run_ctx().deck.get(i))
            .map(|card| RunChoiceCard {instance_id:card.instance_id,card_id:card.id,source_pile:"deck"}).collect();
        let (prompt,min,max,effect,single)=match choice.kind {
            RunDeckChoiceKind::Smith { .. } => ("Choose a card to upgrade",1,1,RunChoiceEffect::Upgrade,true),
            RunDeckChoiceKind::ShopRemove { .. } => ("Choose a card to remove",1,1,RunChoiceEffect::Remove {damage_after:0},true),
            RunDeckChoiceKind::Cook { .. } => ("Choose 2 cards to cook away",2,2,RunChoiceEffect::Remove {damage_after:0},false),
        };
        Some(RunCardChoice {wire_source:Some(match choice.kind {
            RunDeckChoiceKind::Smith { .. } => "rest:smith",
            RunDeckChoiceKind::ShopRemove { .. } => "shop:remove",
            RunDeckChoiceKind::Cook { .. } => "rest:cook",
        }),prompt,options,
            selected:choice.selected.iter().filter_map(|deck| choice.candidates.iter().position(|i| i==deck)).collect(),
            min_choices:min,max_choices:max,allow_skip:false,single_select:single,
            effect,resume:ChoiceResume::MapChoice,reward_cards:Vec::new()})
    }
    /// Read-only transplant inputs, retaining the exact selection toggle order.
    /// Unlike observation_progress_run_choice this never sorts selected indices.
    pub fn observation_ordered_run_choice(&self) -> Option<ObservationRunChoice> {
        let choice = self.observation_run_choice()?;
        Some(ObservationRunChoice {
            source: self.observation_selector_source(),
            prompt: choice.prompt,
            options: choice.options.iter().map(|card| ObservationChoiceCard {
                instance_id: card.instance_id, card_id: card.card_id,
                source_pile: card.source_pile,
            }).collect(),
            selected_in_order: choice.selected.clone(),
            min_choices: choice.min_choices, max_choices: choice.max_choices,
            allow_skip: choice.allow_skip, single_select: choice.single_select,
            is_multi: choice.is_multi(), can_confirm: choice.can_confirm(),
            operation: match choice.effect {
                RunChoiceEffect::AddCards => ObservationChoiceOperation::AddCards,
                RunChoiceEffect::Upgrade => ObservationChoiceOperation::Upgrade,
                RunChoiceEffect::Enchant { name, amount } => ObservationChoiceOperation::Enchant { name: name.name(), amount },
                _ => ObservationChoiceOperation::Unsupported,
            },
            generated_cards: choice.reward_cards,
        })
    }
    /// Unencoded run ownership/history facts needed by readonly binding hashes.
    pub fn observation_selector_ownership(&self) -> (&'static str, usize, Vec<Coord>, bool) {
        let visited = self.visible_map().map_or_else(Vec::new, |map| {
            self.visited_points.iter().map(|&i| map.points[i as usize].coord).collect()
        });
        (self.character.name(), self.player_count, visited, !self.out_of_combat_potions_locked())
    }
    /// Extra-reward classes not represented by the frozen r7 reward menu.
    pub(crate) fn observation_has_unencoded_extra_reward(&self) -> bool {
        self.reward.items.iter().any(|item| !item.consumed && matches!(
            item.kind, RewardItemKind::SpecialCard { .. } | RewardItemKind::CardRemoval { .. }))
    }
    /// Stable source keys for non-card reward kinds whose policy wire is gated.
    pub fn observation_extra_reward_sources(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for item in self.reward.items.iter().filter(|item| !item.consumed) {
            let source = match item.kind {
                RewardItemKind::SpecialCard { .. } => Some("reward:SPECIAL_CARD"),
                RewardItemKind::CardRemoval { .. } => Some("reward:REMOVE_CARD"),
                _ => None,
            };
            if let Some(source) = source && !out.contains(&source) { out.push(source); }
        }
        out
    }
    /// Retained original reward cards; no generation or acquisition side effect.
    pub fn observation_special_card_rewards(&self) -> Vec<(Card, bool)> {
        let skip_allowed = self.reward.items.iter().any(|item| !item.consumed && item.skippable);
        self.ordered_remaining_reward_indices().into_iter().filter_map(|index| {
            match &self.reward.items[index].kind {
                RewardItemKind::SpecialCard { card, .. } => Some((card.clone(), skip_allowed)),
                _ => None,
            }
        }).collect()
    }
    /// Ordered selector inputs for connected replay, including generated cards.
    pub fn observation_selector_cards(&self) -> Option<(Vec<Card>, usize, usize)> {
        let choice = self.observation_run_choice()?;
        let cards = choice.options.iter().map(|option| {
            choice.reward_cards.iter().chain(self.run_ctx().deck.iter())
                .find(|card| card.instance_id == option.instance_id && card.id == option.card_id)
                .expect("selector option must retain its source card").clone()
        }).collect();
        Some((cards, choice.min_choices, choice.max_choices))
    }
    /// Read-only diagnostic context for persistent and projected run selectors.
    pub fn observation_selector_context(&self) -> Option<String> {
        let choice = self.observation_run_choice()?;
        let source = choice.wire_source.unwrap_or("unrecorded");
        Some(format!("{source};effect={:?};prompt={};min={};max={}",
            choice.effect, choice.prompt, choice.min_choices, choice.max_choices))
    }
    /// Stable named selector site used by rule-level observation admission.
    pub fn observation_selector_source(&self) -> Option<&'static str> {
        let choice = self.observation_run_choice()?;
        // SelfHelpBook uses three different C# CanEnchant predicates. In
        // particular, equal Sharp/Swift sets cannot prove the Nimble set:
        // ExpectAFight is a known C#-only Nimble candidate. Keep the literal
        // whole-screen key unchanged, but never share rule-level evidence.
        if choice.wire_source == Some("event.self_help_book.enchant") {
            return match choice.effect {
                RunChoiceEffect::Enchant { name: EnchantmentId::Sharp, amount: 2 } =>
                    Some("event.self_help_book.sharp"),
                RunChoiceEffect::Enchant { name: EnchantmentId::Nimble, amount: 2 } =>
                    Some("event.self_help_book.nimble"),
                RunChoiceEffect::Enchant { name: EnchantmentId::Swift, amount: 2 } =>
                    Some("event.self_help_book.swift"),
                _ => None,
            };
        }
        choice.wire_source
    }
    /// Exact generated Neow offer, without resolving its pickup.
    pub fn native_neow_offers(&self) -> Option<[(super::EventOptionIdentity, RelicId); 3]> {
        (self.phase == RunPhase::Event && self.current_act == 0 && self.run_ctx().total_floor == 1 && self.current_event.is_none())
            .then(|| self.neow.map(|offer| (offer.option_id, offer.relic)))
    }

    pub fn native_event_page(&self) -> Option<super::EventPageIdentity> {
        self.current_event.as_ref().map(|event| event.options.page)
    }

    pub fn native_event_options(&self) -> Option<&[super::NativeEventOption]> {
        self.current_event.as_ref().map(|event| &*event.options)
    }

    /// Compatibility boundary for existing replay/observation consumers.
    pub fn observation_neow_relic(&self, option: &str) -> Option<RelicId> {
        self.neow.iter().find(|n| n.option_id.legacy_id() == option).map(|n| n.relic)
    }
    /// Offline candidate-set certificate only; never an admission override.
    /// The caller must supply the complete ordered instance IDs independently
    /// captured from the legacy selector, then separately prove every wire row.
    pub fn verify_self_help_nimble_candidate_set(&self, legacy: &[u32]) -> Option<Vec<usize>> {
        let choice = self.observation_run_choice()?;
        if choice.wire_source != Some("event.self_help_book.enchant")
            || !matches!(choice.effect, RunChoiceEffect::Enchant { name: EnchantmentId::Nimble, amount: 2 })
            || !choice.single_select || choice.min_choices != 1 || choice.max_choices != 1
            || choice.allow_skip || !choice.selected.is_empty() || !choice.reward_cards.is_empty()
            || choice.prompt != "Choose a card to enchant with Nimble" || legacy.is_empty()
        { return None; }
        // Audited C# SelfHelpBook.SelectAndEnchant restricts to Skill;
        // Nimble.CanEnchant delegates to base then requires GainsBlock.
        // ExpectAFight.cs explicitly overrides GainsBlock => true.
        let expected: Vec<_> = self.run_ctx().deck.iter()
            .filter(|c| c.card_type == CardType::Skill && Self::event_can_enchant(c, "Nimble"))
            .collect();
        if expected.len() != choice.options.len()
            || expected.iter().zip(&choice.options).any(|(card, option)|
                option.source_pile != "deck" || option.instance_id != card.instance_id
                    || option.card_id != card.id)
            || expected.iter().filter(|c| c.id == CardId::ExpectAFight).count() != 1
        { return None; }
        let legacy_expected: Vec<_> = expected.iter().enumerate()
            .filter(|(_, c)| c.id != CardId::ExpectAFight).collect();
        if legacy_expected.len() != legacy.len()
            || legacy_expected.iter().zip(legacy).any(|((_, card), id)| card.instance_id != *id)
            || legacy.iter().collect::<std::collections::BTreeSet<_>>().len() != legacy.len()
        { return None; }
        Some(legacy_expected.into_iter().map(|(index, _)| index).collect())
    }

    pub(crate) fn observation_selector_integrity(&self) -> bool {
        self.observation_run_choice().is_some_and(|c| {
            if c.selected.iter().any(|&i|i>=c.options.len()) {
                return false;
            }
            if c.reward_cards.is_empty() {
                c.options.iter().all(|o| self.run_ctx().deck.iter().any(|card|
                    card.instance_id==o.instance_id && card.id==o.card_id))
            } else {
                c.options.len()==c.reward_cards.len() && c.options.iter().zip(&c.reward_cards).all(|(o,card)|
                    o.instance_id==card.instance_id && o.card_id==card.id)
            }
        })
    }
    pub(crate) fn observation_verified_selector(&self) -> bool {
        self.observation_run_choice().is_some_and(|c|
            self.observation_selector_integrity() && self.recorded_selector_shape(&c))
    }
    /// Canonical recorder key; source identity never depends on source locations.
    pub fn observation_selector_shape(&self) -> Option<String> {
        self.observation_run_choice().map(|c| self.selector_shape_key(&c))
    }
    fn selector_shape_key(&self,c:&RunCardChoice) -> String {
        let source=c.wire_source.expect("selector constructor must name its source");
        assert!(!source.is_empty(), "selector source must not be empty");
        let kinds=c.options.iter().map(|o|o.source_pile).collect::<std::collections::BTreeSet<_>>()
            .into_iter().collect::<Vec<_>>().join(",");
        let ctx=self.run_ctx();
        // Exact ordered candidates, selected indices, prompt/purpose and phase,
        // plus all noncombat state-row inputs determining literal source routes.
        // Counts alone must never let a different C# option set borrow a proof.
        let mut key=format!("screen-v2;source={source};options={kinds};screen={:?}",
            (self.phase(),self.current_event_id(),self.public_act1_boss(),c,
                (ctx.hp,ctx.max_hp,ctx.gold,ctx.total_floor,ctx.ascension,ctx.act_index),
                &ctx.deck,&ctx.potions,&ctx.relics));
        // Inventory-augmented screens require a distinct witness of the
        // complete executable candidate order, not a Choose-only certificate.
        let mut copy=self.clone();
        let actions=copy.available_actions();
        if actions.iter().any(|a|matches!(a.kind,RunActionKind::UsePotion|RunActionKind::DiscardPotion)) {
            key.push_str(&format!(";inventory-candidates-v1={actions:?}"));
        }
        key
    }
    fn recorded_selector_shape(&self,c:&RunCardChoice) -> bool {
        if c.selected.iter().any(|&i|i>=c.options.len()) {return false;}
        recorded_selector_key(&self.selector_shape_key(c))
    }
    /// Read-only identity of a generated card offer, stable across modal closes.
    pub fn observation_reward_card_identity(&self, index: usize) -> Option<u64> {
        let item = *self.ordered_remaining_reward_indices().get(index)?;
        match &self.reward.items.get(item)?.kind {
            RewardItemKind::Card { cards, .. } => cards.first().map(|c| u64::from(c.instance_id)),
            _ => None,
        }
    }
    /// Presentation-independent remaining rewards and selected deck options.
    /// Frozen StructuredEnv._reward_semantic_key/_pending_choice_semantic_key.
    pub fn observation_progress_menu(&self) -> Vec<(&'static str, &'static str, &'static str, u32, u32)> {
        let mut rewards: Vec<_> = self.reward.items.iter().filter(|i| !i.consumed).map(|i| match &i.kind {
            RewardItemKind::Potion(id) => ("POTION", id.name(), "", 0, 0),
            RewardItemKind::Relic(id) => ("RELIC", "", id.name(), 0, 0),
            RewardItemKind::Card { progress, .. } => ("CARD", "", "", progress.cards_picked, progress.rerolls_used),
            RewardItemKind::CardBundles { .. } => ("CARD_BUNDLE", "", "", 0, 0),
            RewardItemKind::SpecialCard { .. } => ("ADD_CARD", "", "", 0, 0),
            RewardItemKind::CardRemoval { .. } => ("REMOVE_CARD", "", "", 0, 0),
        }).collect();
        rewards.sort_unstable();
        rewards
    }
    /// All reward objects, including consumed ones, for counter auditing.
    /// Stable ordinary-card ordinal, including canceled but unconsumed rewards.
    pub fn observation_active_card_reward_ordinal(&self) -> Option<usize> {
        let active=self.reward.active?;
        Some(self.reward.items[..active].iter()
            .filter(|item|matches!(item.kind,RewardItemKind::Card{..})).count())
    }

    pub fn observation_card_reward_skip_completes(&self) -> bool {
        self.reward.active.and_then(|index|self.reward.items.get(index))
            .is_some_and(|item|item.skip_completes)
    }

    /// Stable PotionReward ordinal in the retained original RewardsSet.
    pub fn observation_potion_reward_ordinal(&self, remaining_index:Option<usize>) -> Option<usize> {
        let selected=match remaining_index {
            Some(index)=>*self.ordered_remaining_reward_indices().get(index)?,
            None=>self.reward.active?,
        };
        if !matches!(self.reward.items.get(selected)?.kind,RewardItemKind::Potion(_)) {return None;}
        Some(self.reward.items[..selected].iter()
            .filter(|item|matches!(item.kind,RewardItemKind::Potion(_))).count())
    }

    pub fn observation_card_reward_counters(&self) -> Vec<(u32,u32)> {
        self.reward.items.iter().filter_map(|item| match &item.kind {
            RewardItemKind::Card { progress, .. } => Some((progress.cards_picked,progress.rerolls_used)),
            _ => None,
        }).collect()
    }
    /// Full source-card snapshots, not only model IDs; reads no RNG.
    pub fn observation_progress_run_choice(&self) -> Result<Option<(Vec<usize>,Vec<(&'static str,Card)>)>, &'static str> {
        let (mut selected, options) = if let Some(c) = &self.pending_run_choice {
            let mut cards=Vec::new();
            for option in &c.options {
                let card=c.reward_cards.iter().chain(self.run_ctx().deck.iter())
                    .find(|card| card.instance_id == option.instance_id)
                    .ok_or("progress choice option card is missing")?;
                cards.push((option.source_pile,card.clone()));
            }
            (c.selected.clone(),cards)
        } else if let Some(c) = &self.run_choice {
            let mut cards=Vec::new();
            for &index in &c.candidates {
                cards.push(("deck",self.run_ctx().deck.get(index)
                    .ok_or("progress deck choice index is missing")?.clone()));
            }
            (c.selected.clone(),cards)
        } else { return Ok(None) };
        selected.sort_unstable();
        Ok(Some((selected,options)))
    }
    /// EventState has no independent pending-choice owner in this engine:
    /// event grids are registered in pending_run_choice, as in r7 run_state.
    pub fn observation_progress_event_choice(&self) -> Option<(Vec<usize>,Vec<(&'static str,Card)>)> {
        None
    }
    /// TheArchitect's Proceed completes victory; event deaths retain hp <= 0.
    pub fn observation_full_run_victory(&self) -> bool {
        self.phase == RunPhase::RunOver && self.run_ctx().hp > 0
            && self.current_act + 1 >= self.acts.len() && self.unported.is_none()
    }
    #[inline]
    fn run_ctx_mut(&mut self) -> &mut RunCtx {
        if let Some(c) = self.combat.as_mut() {
            &mut c.run
        } else {
            self.run.as_mut().expect("run context")
        }
    }

    pub fn available_actions(&mut self) -> Vec<RunAction> {
        if self.phase==RunPhase::Shop&&self.pending_run_choice.is_none()&&self.parasol_next_relic.is_some() {
            if let Err(reason)=self.auto_purchase_shop_inventory(){self.unported=Some(reason);}
        }
        let mut out = Vec::new();
        // sim_r1_events: run-level card choices pre-empt every non-combat phase.
        if self.phase != RunPhase::Combat && self.pending_run_choice.is_some() {
            self.pending_run_choice_actions(&mut out);
            self.out_of_combat_potion_actions(&mut out);
            return out;
        }
        match self.phase {
            RunPhase::Event if self.current_act == 0 && self.run_ctx().total_floor == 1 => {
                for choice in self.neow {
                    let mut a = RunAction::new(RunActionKind::EventChoice);
                    a.option_id = Some(choice.option_id.legacy_id());
                    a.event_option_id = Some(choice.option_id);
                    a.label = Some(neow_label(choice.relic));
                    a.description = Some(if choice.option_id.offer_index(EventOptionId::NeowPageNameRelicIdEntry) == Some(2) {
                        "Gain a cursed relic"
                    } else {
                        "Gain a positive relic"
                    });
                    out.push(a);
                }
            }
            RunPhase::MapChoice => {
                self.out_of_combat_potion_actions(&mut out);
                let map = self.acts[self.current_act].map.as_ref().unwrap();
                // run: run/run_state.py:1948
                if self.visited_points.is_empty() {
                    let p = &map.points[map.start as usize];
                    push_move_action(&mut out, p.coord, p.point_type);
                } else if self.winged_boots_free_travel() {
                    let next_row = map.points[self.current_point as usize].coord.row + 1;
                    for col in 0..super::map::MAP_WIDTH {
                        // C# MapTravel: removed points are not destinations.
                        if let Some(p) = map.point(Coord::new(col, next_row)) {
                            push_move_action(&mut out, p.coord, p.point_type);
                        }
                    }
                } else {
                    for &child in &map.points[self.current_point as usize].children {
                        let p = &map.points[child as usize];
                        push_move_action(&mut out, p.coord, p.point_type);
                    }
                }
            }
            RunPhase::Combat => self.combat_actions(&mut out),
            RunPhase::CardReward => {
                self.reward_actions(&mut out);
                self.out_of_combat_potion_actions(&mut out);
            }
            RunPhase::Treasure => {
                let mut a = RunAction::new(RunActionKind::Collect);
                a.relic_id = self.treasure_relic;
                out.push(a);
                self.out_of_combat_potion_actions(&mut out);
            }
            RunPhase::Shop => {
                if self.run_choice.is_some() {
                    self.run_choice_actions(&mut out);
                    self.out_of_combat_potion_actions(&mut out);
                } else {
                    self.shop_actions(&mut out);
                    self.foul_potion_shop_actions(&mut out);
                }
            }
            RunPhase::RestSite => {
                if self.run_choice.is_some() {
                    self.run_choice_actions(&mut out);
                } else {
                    self.rest_actions(&mut out);
                }
                self.out_of_combat_potion_actions(&mut out);
            }
            RunPhase::RunOver => {}
            RunPhase::Event => {
                if let Some(event) = self.current_event.as_ref() {
                    for option in &event.options {
                        // Locked buttons are visible UI, not legal actions.
                        if !option.enabled { continue; }
                        let mut action = RunAction::new(RunActionKind::EventChoice);
                        action.option_id = Some(option.option_id.legacy_id());
                        action.event_option_id = Some(option.option_id);
                        action.label = Some(option.label);
                        action.description = if option.dynamic_description.is_some() {
                            None
                        } else {
                            Some(option.description)
                        };
                        action.dynamic_description = option.dynamic_description;
                        action.enabled = option.enabled;
                        if event.id == EventId::FakeMerchant {
                            if let Some(index) = event.options.iter().position(|o| o.option_id == option.option_id)
                                .filter(|&i| i < event.relics.len()) {
                                action.price = Some(event.ints[index]);
                                action.relic_id = Some(event.relics[index]);
                            }
                        }
                        if (option.option_id.is(EventOptionId::LuminousChoirInitialOfferTribute) && option.enabled)
                            || option.option_id.is(EventOptionId::WhisperingHollowInitialGold)
                            || option.option_id.is(EventOptionId::CrystalSphereInitialUncoverFuture) {
                            action.price = Some(event.ints[0]);
                        }
                        out.push(action);
                    }
                    self.out_of_combat_potion_actions(&mut out);
                    if event.id == EventId::FakeMerchant { self.foul_potion_shop_actions(&mut out); }
                }
            }
        }
        out
    }

    fn pending_run_choice_actions(&self, out: &mut Vec<RunAction>) {
        let Some(choice) = self.pending_run_choice.as_ref() else {
            return;
        };
        if choice.is_multi() {
            if choice.can_confirm() {
                let mut action = RunAction::new(RunActionKind::ConfirmChoice);
                action.prompt = Some(choice.prompt);
                action.selected_count = Some(choice.selected.len());
                out.push(action);
            }
        } else if choice.allow_skip {
            out.push(RunAction::new(RunActionKind::ConfirmChoice));
        }
        if choice.single_select && !choice.reward_cards.is_empty() {
            // BrainLeech.ShareKnowledge uses noncancelable CardSelectorPrefs(1),
            // whose default RequireManualConfirmation is false.
            for (i, card) in choice.reward_cards.iter().enumerate() {
                let mut action = RunAction::new(RunActionKind::PickCard);
                action.index = Some(i);
                action.card_id = Some(card.id);
                action.rarity = Some(card.rarity);
                action.upgraded = Some(card.upgraded);
                action.enchantment_0 = card.enchantments.first().map(|&(id, amount)| (id.name(), amount));
                action.enchantment_1 = card.enchantments.get(1).map(|&(id, amount)| (id.name(), amount));
                out.push(action);
            }
            return;
        }
        for (i, option) in choice.options.iter().enumerate() {
            if !choice.can_toggle(i) {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::Choose);
            a.index = Some(i);
            a.card_id = Some(option.card_id);
            a.source_pile = Some(option.source_pile);
            a.selected = Some(choice.selected.contains(&i));
            out.push(a);
        }
    }

    fn run_choice_actions(&self, out: &mut Vec<RunAction>) {
        let Some(choice) = self.run_choice.as_ref() else {
            return;
        };
        let at_max =
            matches!(choice.kind, RunDeckChoiceKind::Cook { .. }) && choice.selected.len() >= 2;
        if at_max {
            let mut confirm = RunAction::new(RunActionKind::ConfirmChoice);
            confirm.prompt = Some("Choose 2 cards to cook away");
            confirm.selected_count = Some(choice.selected.len());
            out.push(confirm);
        }
        let run = self.run_ctx();
        for (option_index, &deck_index) in choice.candidates.iter().enumerate() {
            let selected = choice.selected.contains(&deck_index);
            if at_max && !selected {
                continue;
            }
            let Some(card) = run.deck.get(deck_index) else {
                continue;
            };
            let mut a = RunAction::new(RunActionKind::Choose);
            a.index = Some(option_index);
            a.card_id = Some(card.id);
            a.source_pile = Some("deck");
            a.selected = Some(selected);
            out.push(a);
        }
    }

    fn rest_actions(&self, out: &mut Vec<RunAction>) {
        for option in &self.rest.options {
            if !option.enabled {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::RestOption);
            a.option_id = Some(option.kind.legacy_id());
            a.rest_option = Some(option.kind);
            a.label = Some(option.label);
            a.description = Some(option.description);
            a.enabled = option.enabled;
            out.push(a);
        }
    }

    fn shop_actions(&self, out: &mut Vec<RunAction>) {
        out.push(RunAction::new(RunActionKind::LeaveShop));
        self.out_of_combat_potion_actions(out);
        let gold = self.run_ctx().gold;
        for (index, entry) in self
            .shop
            .cards
            .iter()
            .chain(self.shop.colorless_cards.iter())
            .enumerate()
        {
            let Some(card) = entry.card.as_ref() else {
                continue;
            };
            if gold < entry.price {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::BuyCard);
            a.index = Some(index);
            a.price = Some(entry.price);
            a.card_id = Some(card.id);
            a.rarity = Some(card.rarity);
            a.card_type_name = Some(entry.card_type);
            a.upgraded = Some(card.upgraded);
            a.on_sale = Some(entry.on_sale);
            a.enchantment_0 = card.enchantments.first().map(|&(id, amount)| (id.name(), amount));
            a.enchantment_1 = card.enchantments.get(1).map(|&(id, amount)| (id.name(), amount));
            out.push(a);
        }
        for (index, entry) in self.shop.relics.iter().enumerate() {
            if gold < entry.price || entry.price >= shop::SOLD_OUT {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::BuyRelic);
            a.index = Some(index);
            a.price = Some(entry.price);
            a.relic_id = Some(entry.relic_id);
            a.relic_rarity = Some(entry.rarity);
            out.push(a);
        }
        for (index, entry) in self.shop.potions.iter().enumerate() {
            // A failed TryToProcure is not a purchasable policy action,
            // just as an unaffordable/out-of-stock entry is not one.
            if gold < entry.price || entry.price >= shop::SOLD_OUT
                || !self.can_buy_shop_potion()
            {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::BuyPotion);
            a.index = Some(index);
            a.price = Some(entry.price);
            a.potion_id = Some(entry.potion_id);
            a.potion_rarity = Some(entry.rarity);
            out.push(a);
        }
        if !self.shop.removal_used
            && gold >= self.shop.removal_cost
            && !rest::removable_indices(self.run_ctx()).is_empty()
        {
            let mut a = RunAction::new(RunActionKind::RemoveCard);
            a.price = Some(self.shop.removal_cost);
            out.push(a);
        }
    }

    fn foul_potion_shop_actions(&self, out: &mut Vec<RunAction>) {
        for (slot, potion) in self.run_ctx().potions.iter().copied().enumerate() {
            if potion != Some(PotionId::Foulpotion) {
                continue;
            }
            let mut a = RunAction::new(RunActionKind::UsePotion);
            a.slot_index = Some(slot);
            a.potion_id = Some(PotionId::Foulpotion);
            out.push(a);
        }
    }

    fn out_of_combat_potion_actions(&self, out: &mut Vec<RunAction>) {
        if self.out_of_combat_potions_locked() { return; }
        for (slot, p) in self.run_ctx().potions.iter().copied().enumerate() {
            let Some(id) = p else { continue };
            let info = &POTION_INFO[id.index()];
            if info.usage == PotionUsageType::AnyTime && id != PotionId::Foulpotion {
                let mut a = RunAction::new(RunActionKind::UsePotion);
                a.slot_index = Some(slot);
                a.potion_id = Some(id);
                if info.target == PotionTargetType::AnyPlayer {
                    a.target_player_id = Some(PLAYER_ID);
                }
                out.push(a);
            }
            let mut a = RunAction::new(RunActionKind::DiscardPotion);
            a.slot_index = Some(slot);
            a.potion_id = Some(id);
            out.push(a);
        }
    }

    fn combat_actions(&mut self, out: &mut Vec<RunAction>) {
        let Some(c) = self.combat.as_mut() else {
            return;
        };
        if c.is_over {
            return;
        }
        if let Some(choice) = c.pending_choice.as_ref() {
            if choice.is_multi() {
                let count = c.choice_selected.len() as u16;
                if count >= choice.min || (choice.allow_skip && count == 0) {
                    out.push(RunAction::new(RunActionKind::ConfirmChoice));
                }
            } else if choice.allow_skip {
                out.push(RunAction::new(RunActionKind::ConfirmChoice));
            }
            for (i, &k) in c.choice_options.iter().enumerate() {
                let selected = c.choice_selected.contains(&(i as u16));
                if !selected && choice.is_multi() && c.choice_selected.len() as u16 >= choice.max {
                    continue;
                }
                let card = &c.cards[k as usize];
                let mut a = RunAction::new(RunActionKind::Choose);
                a.index = Some(i);
                a.card_id = Some(card.id);
                a.cost = Some(card.cost);
                a.selected = Some(selected);
                a.source_pile = Some(match choice.source {
                    crate::combat::PileKind::Hand => "hand",
                    crate::combat::PileKind::Draw => "draw",
                    crate::combat::PileKind::Discard => "discard",
                    crate::combat::PileKind::Exhaust => "exhaust",
                    crate::combat::PileKind::Play => "play",
                    crate::combat::PileKind::None => "none",
                });
                out.push(a);
            }
            return;
        }
        out.push(RunAction::new(RunActionKind::EndTurn));
        for (slot, p) in c.run.potions.iter().copied().enumerate() {
            let Some(id) = p else { continue };
            let info = &POTION_INFO[id.index()];
            if info.target == PotionTargetType::AnyEnemy {
                for (i, &enemy) in c.enemies.iter().enumerate() {
                    if c.cr(enemy).is_alive() && c.can_use_potion(slot, Some(i)) {
                        let mut a = RunAction::new(RunActionKind::UsePotion);
                        a.player_id = Some(PLAYER_ID);
                        a.slot_index = Some(slot);
                        a.potion_id = Some(id);
                        a.target_index = Some(i);
                        a.target_name = Some(c.cr(enemy).monster_id);
                        out.push(a);
                    }
                }
            } else if c.can_use_potion(slot, None) {
                let mut a = RunAction::new(RunActionKind::UsePotion);
                a.player_id = Some(PLAYER_ID);
                a.slot_index = Some(slot);
                a.potion_id = Some(id);
                out.push(a);
            }
        }
        for i in 0..c.players[crate::combat::PLAYER].hand.len() {
            let k = c.players[crate::combat::PLAYER].hand[i];
            if !c.can_play_card(k) {
                continue;
            }
            let card = &c.cards[k as usize];
            let (card_id, cost, target) = (card.id, card.cost, c.card_target_type(k));
            if target == TargetType::AnyEnemy {
                for (j, &enemy) in c.enemies.iter().enumerate() {
                    if !c.cr(enemy).is_alive() {
                        continue;
                    }
                    let mut a = RunAction::new(RunActionKind::PlayCard);
                    a.player_id = Some(PLAYER_ID);
                    a.hand_index = Some(i);
                    a.card_id = Some(card_id);
                    a.cost = Some(cost);
                    a.target_index = Some(j);
                    a.target_name = Some(c.cr(enemy).monster_id);
                    out.push(a);
                }
            } else {
                let mut a = RunAction::new(RunActionKind::PlayCard);
                a.player_id = Some(PLAYER_ID);
                a.hand_index = Some(i);
                a.card_id = Some(card_id);
                a.cost = Some(cost);
                out.push(a);
            }
        }
    }

    fn reward_actions(&self, out: &mut Vec<RunAction>) {
        if self.awaiting_next_act {
            let mut a = RunAction::new(RunActionKind::Skip);
            a.transition = Some("next_act");
            out.push(a);
            return;
        }

        let active_card = self.reward.active.is_some_and(|index| {
            matches!(
                self.reward.items.get(index).map(|item| &item.kind),
                Some(RewardItemKind::Card { closed: false, .. })
                    | Some(RewardItemKind::CardBundles { .. })
            )
        });
        if !active_card {
            self.reward_container_actions(out);
        }

        let Some(active) = self.reward.active else {
            return;
        };
        let Some(item) = self.reward.items.get(active) else {
            return;
        };
        match &item.kind {
            RewardItemKind::Card {
                cards,
                closed: false,
                reroll,
                ..
            } => {
                if item.skippable {
                    out.push(RunAction::new(RunActionKind::Skip));
                }
                // C# CardRewardAlternative.Generate: Skip, then one-shot REROLL.
                if reroll.is_some() && self.run_ctx().owned_relics(crate::combat::PLAYER)
                    .any(|r| r.id == RelicId::Driftwood && r.enabled)
                {
                    out.push(RunAction::new(RunActionKind::RerollCardReward));
                }
                // Frozen r7 run_manager.py:_card_reward_alternatives and
                // relics/shop_event.py:PaelsWing add this one-shot alternative.
                if reroll.is_some() && self.run_ctx().owned_relics(crate::combat::PLAYER)
                    .any(|r| r.id == RelicId::PaelsWing && r.enabled)
                {
                    out.push(RunAction::new(RunActionKind::SacrificeCardReward));
                }
                for (i, card) in cards.iter().enumerate() {
                    let mut a = RunAction::new(RunActionKind::PickCard);
                    a.index = Some(i);
                    a.card_id = Some(card.id);
                    a.rarity = Some(card.rarity);
                    a.upgraded = Some(card.upgraded);
                    a.enchantment_0 = card.enchantments.first().map(|&(id, amount)| (id.name(), amount));
                    a.enchantment_1 = card.enchantments.get(1).map(|&(id, amount)| (id.name(), amount));
                    out.push(a);
                }
            }
            RewardItemKind::Potion(id) => {
                let mut skip = RunAction::new(RunActionKind::SkipPotion);
                skip.potion_id = Some(*id);
                out.push(skip);
                if self.can_procure_potion() {
                    let mut pick = RunAction::new(RunActionKind::PickPotion);
                    pick.potion_id = Some(*id);
                    out.push(pick);
                }
            }
            RewardItemKind::Relic(id) => {
                if item.skippable {
                    let mut skip = RunAction::new(RunActionKind::SkipRelic);
                    skip.relic_id = Some(*id);
                    out.push(skip);
                }
                let mut pick = RunAction::new(RunActionKind::PickRelicReward);
                pick.relic_id = Some(*id);
                out.push(pick);
            }
            RewardItemKind::CardBundles { bundles } => {
                for (i, bundle) in bundles.iter().enumerate() {
                    let mut action = RunAction::new(RunActionKind::PickCardBundle);
                    action.index = Some(i);
                    action.bundle_len = bundle.len() as u8;
                    for (slot, card) in bundle.iter().enumerate() {
                        action.bundle_card_ids[slot] = Some(card.id);
                        action.bundle_rarities[slot] = Some(card.rarity);
                        action.bundle_upgraded[slot] = card.upgraded;
                    }
                    out.push(action);
                }
            }
            RewardItemKind::Card { closed: true, .. } => {}
            RewardItemKind::SpecialCard { .. } | RewardItemKind::CardRemoval { .. } => {}
        }
    }

    // run: run/run_manager.py:1393
    fn reward_container_actions(&self, out: &mut Vec<RunAction>) {
        let active = self.reward.active;
        let ordered_indices = self.ordered_remaining_reward_indices();
        for (reward_index, item_index) in ordered_indices.iter().copied().enumerate() {
            let item = &self.reward.items[item_index];
            if Some(item_index) != active {
                match &item.kind {
                    RewardItemKind::Potion(id) if self.can_procure_potion() => {
                        let mut action = RunAction::new(RunActionKind::PickPotion);
                        action.reward_index = Some(reward_index);
                        action.reward_type = Some("POTION");
                        action.potion_id = Some(*id);
                        out.push(action);
                    }
                    RewardItemKind::Relic(id) => {
                        let mut action = RunAction::new(RunActionKind::PickRelicReward);
                        action.reward_index = Some(reward_index);
                        action.reward_type = Some("RELIC");
                        action.relic_id = Some(*id);
                        out.push(action);
                    }
                    RewardItemKind::Card { .. } => {
                        let mut action = RunAction::new(RunActionKind::OpenReward);
                        action.reward_index = Some(reward_index);
                        action.reward_type = Some("CARD");
                        action.selected = Some(false);
                        out.push(action);
                    }
                    RewardItemKind::SpecialCard { card, .. } => {
                        let mut action = RunAction::new(RunActionKind::PickSpecialCard);
                        action.reward_index = Some(reward_index);
                        action.reward_type = Some("SPECIAL_CARD");
                        action.card_id = Some(card.id);
                        out.push(action);
                    }
                    RewardItemKind::CardRemoval { .. } => {
                        let mut action = RunAction::new(RunActionKind::OpenReward);
                        action.reward_index = Some(reward_index);
                        action.reward_type = Some("REMOVE_CARD");
                        action.selected = Some(false);
                        out.push(action);
                    }
                    RewardItemKind::CardBundles { .. } | RewardItemKind::Potion(_) => {}
                }
            }
        }
        if ordered_indices.iter().any(|&i| self.reward.items[i].skippable) {
            out.push(RunAction::new(RunActionKind::SkipRewards));
        }
    }

    fn can_procure_potion(&self) -> bool {
        let run = self.run_ctx();
        run.potions.iter().filter(|p| p.is_some()).count() < run.max_potion_slots as usize
    }

    pub(super) fn can_buy_shop_potion(&self)->bool {
        self.can_procure_potion()
            && !self.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Sozu)
    }

    /// Apply the selected Neow relic and its run-layer pickup effect. Relic pickup effects that
    /// need a reward/choice flow not implemented here remain explicitly fail-closed.
    pub fn apply_recorded_neow_outcome_for_parity(
        &mut self,
        option_id: &str,
    ) -> Result<(), &'static str> {
        let option = self.neow.iter().find(|choice| choice.option_id.legacy_id() == option_id)
            .map(|choice| choice.option_id).ok_or("unknown Neow option")?;
        self.resolve_neow_choice(option)
    }

    fn resolve_neow_choice(&mut self, option_id: EventOptionIdentity) -> Result<(), &'static str> {
        if self.phase != RunPhase::Event || self.run_ctx().total_floor != 1 {
            return Err("not at Neow");
        }
        let choice = self
            .neow
            .iter()
            .find(|x| x.option_id == option_id)
            .copied()
            .ok_or("unknown Neow option")?;
        // The startup marker describes the unresolved choice, not a failed pickup.
        // Clear it before invoking hooks so only an actual hook failure stops the chain.
        self.unported = None;
        self.remove_relic_from_bags(choice.relic)?;
        let mut relic = Relic::new(choice.relic);
        // C# RelicCmd.Obtain assigns RunState.TotalFloor before AfterObtained.
        relic.floor_added_to_deck = self.run_ctx().total_floor;
        self.run_ctx_mut().relics.push(relic);
        self.apply_neow_pickup(choice.relic)
    }

    fn apply_neow_pickup(&mut self, relic: RelicId) -> Result<(), &'static str> {
        match relic {
            // relics/shop_event.py:892-927 — one uniform, non-upgraded rare
            // Ironclad card, selected from the run's Rewards stream.
            RelicId::ArcaneScroll => {
                // C# ArcaneScroll.AfterObtained uses the owner's unlocked pool.
                let candidates: Vec<CardId> = super::card_pools::cards(self.character.into(), self.is_multiplayer())?
                    .into_iter()
                    .filter(|id| {
                        Card::from_factory(*id, false, 0)
                            .is_some_and(|card| card.rarity == CardRarity::Rare)
                    })
                    .collect();
                if !candidates.is_empty() {
                    let id = {
                        let run = self.run_ctx_mut();
                        candidates[run.rngs.rewards.choice_index(candidates.len())]
                    };
                    self.add_card_to_deck(id)?;
                }
            }
            // relics/ancient_new.py:102-108
            RelicId::DowsingRod => {
                self.add_card_to_deck(CardId::from_name("DOWSING").ok_or("Dowsing card missing")?)?
            }
            // relics/ancient_new.py:258-264
            RelicId::NeowsSacrifice => {
                let run = self.run_ctx_mut();
                if run.potions.len() < run.max_potion_slots as usize {
                    run.potions.push(Some(PotionId::Ambergris));
                }
                self.add_card_to_deck(CardId::Guilty)?;
            }
            // relics/ancient_new.py:270-276
            RelicId::NeowsTalisman => {
                let (strike, defend) = {
                    let deck = &self.run_ctx().deck;
                    let strike = deck.iter().rposition(|card| {
                        card.rarity == CardRarity::Basic && card.has_tag(CardTag::Strike)
                    });
                    let defend = deck.iter().rposition(|card| {
                        card.rarity == CardRarity::Basic && card.has_tag(CardTag::Defend)
                    });
                    (strike, defend)
                };
                for index in [strike, defend].into_iter().flatten() {
                    self.run_ctx_mut().upgrade_deck_card(index);
                }
            }
            // LeadPaperweight.cs AfterObtained awaits a skippable
            // FromChooseACardScreen, not a CardReward in a RewardsSet.
            RelicId::LeadPaperweight => {
                self.offer_lead_paperweight_choice(2)?;
                self.unported = None;
                return Ok(());
            }
            // PhialHolster.AfterObtained calls the two-potion factory once:
            // owner's epoch + shared source order, sampling without replacement.
            // Both draws happen before either TryToProcure, even with a full belt.
            RelicId::PhialHolster => {
                self.run_ctx_mut().max_potion_slots += 1;
                let mut pool=super::potion_pools::cards(self.character)
                    .ok_or("Phial Holster owner potion pool missing")?;
                let mut potions=Vec::new();
                for _ in 0..2 {
                    let rarity=shop::roll_potion_rarity(&mut self.run_ctx_mut().rngs.combat_potion_generation);
                    let candidates:Vec<_>=pool.iter().enumerate()
                        .filter_map(|(index,id)|(POTION_INFO[id.index()].rarity==rarity).then_some(index)).collect();
                    let pick=self.run_ctx_mut().rngs.combat_potion_generation.choice_index(candidates.len());
                    potions.push(pool.remove(candidates[pick]));
                }
                for potion in potions {
                    let _ = shop::procure_potion(self.run_ctx_mut(), potion);
                }
            }
            // relics/shop_event.py:2856-2868.
            RelicId::PreciseScissors => {
                let candidates = self.removable_deck_indices();
                if !candidates.is_empty() {
                    self.begin_run_card_choice(
                        "relic:PRECISE_SCISSORS",
                        "Choose a card to remove",
                        &candidates,
                        1,
                        1,
                        false,
                        RunChoiceEffect::Remove { damage_after: 0 },
                        ChoiceResume::MapChoice,
                    );
                    self.unported = None;
                    return Ok(());
                }
            }
            // relics/shop_event.py:2810-2824.
            RelicId::Pomander => {
                let candidates = self.upgradable_deck_indices();
                if !candidates.is_empty() {
                    self.begin_run_card_choice(
                        "relic:POMANDER",
                        "Choose a card to upgrade",
                        &candidates,
                        1,
                        1,
                        false,
                        RunChoiceEffect::Upgrade,
                        ChoiceResume::MapChoice,
                    );
                    self.unported = None;
                    return Ok(());
                }
            }
            // relics/shop_event.py:2828-2852.
            RelicId::PrecariousShears => {
                let candidates = self.removable_deck_indices();
                let count = 2.min(candidates.len());
                if count > 0 {
                    self.begin_run_card_choice(
                        "relic:PRECARIOUS_SHEARS",
                        "Choose 2 cards to remove",
                        &candidates,
                        count,
                        count,
                        false,
                        RunChoiceEffect::Remove { damage_after: 16 },
                        ChoiceResume::MapChoice,
                    );
                    self.unported = None;
                    return Ok(());
                }
                self.apply_event_effect(EventEffect::LoseHp(16))?;
                if self.run_ctx().hp == 0 {
                    self.unported = None;
                    return Ok(());
                }
            }
            // relics/ancient_new.py:21-55. Three uniform rare cards, no upgrade
            // roll; resolving or skipping the reward adds Injury afterwards.
            RelicId::HeftyTablet => {
                self.reward = RewardState::default();
                let item =
                    self.make_uniform_card_reward(3, self.character.into(), CardRarity::Rare, true, true)?;
                self.reward.items.push(item);
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
                self.unported = None;
                return Ok(());
            }
            // relics/shop_event.py:2265-2274.
            RelicId::LostCoffer => {
                self.reward = RewardState::default();
                let card =
                    self.make_noncombat_card_reward(3, self.character.into(), None, true, false, false)?;
                self.reward.items.push(card);
                let potion = self.roll_potion().ok_or("unsupported potion reward pool")?;
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Potion(potion),
                    consumed: false,
                    skippable: true,
                    skip_completes: true,
                    add_injury_after: false,
                    neows_bones_final: false,
                });
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
                self.unported = None;
                return Ok(());
            }
            // relics/shop_event.py:3243-3250.
            RelicId::SmallCapsule => {
                self.reward = RewardState::default();
                let relic = self.pull_player_relic(None, false);
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Relic(relic),
                    consumed: false,
                    skippable: true,
                    skip_completes: true,
                    add_injury_after: false,
                    neows_bones_final: false,
                });
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
                self.unported = None;
                return Ok(());
            }
            // relics/shop_event.py:3060-3083; run_state.py:1280-1313.
            RelicId::ScrollBoxes => {
                self.offer_neow_card_bundles()?;
                self.unported = None;
                return Ok(());
            }
            // relics/shop_event.py:2157-2167.
            RelicId::LargeCapsule => {
                // LargeCapsule.GetStrikeForCharacter/GetDefendForCharacter:
                // first Basic bearing each tag in Owner.Character.CardPool.
                let pool = super::card_pools::cards(self.character.into(), self.is_multiplayer())?;
                let basic = |tag| pool.iter().copied().find(|&id|
                    Card::from_factory(id, false, 0).is_some_and(|c|
                        c.rarity == CardRarity::Basic && c.has_tag(tag)));
                let strike = basic(CardTag::Strike).ok_or("LargeCapsule owner Strike missing")?;
                let defend = basic(CardTag::Defend).ok_or("LargeCapsule owner Defend missing")?;
                for _ in 0..2 {
                    let id = self.pull_player_relic(None, false);
                    self.obtain_event_relic(id)?;
                    if self.unported.is_some() {
                        return Ok(());
                    }
                }
                self.add_card_to_deck(strike)?;
                self.add_card_to_deck(defend)?;
            }
            // relics/shop_event.py:2219-2233.
            RelicId::LeafyPoultice => {
                {
                    let run = self.run_ctx_mut();
                    run.max_hp = (run.max_hp - 12).max(1);
                    run.hp = run.hp.min(run.max_hp);
                }
                let basics: Vec<u32> = self
                    .run_ctx()
                    .deck
                    .iter()
                    .filter(|card| card.rarity == CardRarity::Basic)
                    .filter(|card| !card.has_kw(KW_ETERNAL))
                    .filter(|card| card.has_tag(CardTag::Strike) || card.has_tag(CardTag::Defend))
                    .map(|card| card.instance_id)
                    .collect();
                let strike = self
                    .run_ctx()
                    .deck
                    .iter()
                    .find(|card| {
                        basics.contains(&card.instance_id) && card.has_tag(CardTag::Strike)
                    })
                    .map(|card| card.instance_id);
                let defend = self
                    .run_ctx()
                    .deck
                    .iter()
                    .find(|card| {
                        basics.contains(&card.instance_id) && card.has_tag(CardTag::Defend)
                    })
                    .map(|card| card.instance_id);
                for instance_id in [strike, defend].into_iter().flatten() {
                    self.transform_deck_card_with_stream(instance_id, true)?;
                }
            }
            // relics/shop_event.py:2420-2431 — New Leaf offers a non-skippable
            // transform choice over the deck's transformable instances.
            RelicId::NewLeaf => {
                let candidates = self.transformable_deck_indices();
                if !candidates.is_empty() {
                    self.begin_run_card_choice(
                        "relic:NEW_LEAF",
                        "Choose a card to transform",
                        &candidates,
                        1,
                        1,
                        false,
                        RunChoiceEffect::Transform,
                        ChoiceResume::MapChoice,
                    );
                    self.unported = None;
                    return Ok(());
                }
            }
            // relics/ancient_new.py:300-315
            RelicId::SilkenTress => self.run_ctx_mut().gold = 0,
            // relics/shop_event.py:1387-1394
            RelicId::CursedPearl => {
                self.add_card_to_deck(CardId::from_name("GREED").ok_or("Greed card missing")?)?;
                treasure::gain_gold(self.run_ctx_mut(), 333);
            }
            // relics/shop_event.py:2048-2056
            RelicId::GoldenPearl => { treasure::gain_gold(self.run_ctx_mut(), 150); }
            // relics/shop_event.py:2408-2415
            RelicId::NeowsTorment => self.add_card_to_deck(
                CardId::from_name("NEOWS_FURY").ok_or("NeowsFury card missing")?,
            )?,
            // relics/shop_event.py:2436-2444
            RelicId::NutritiousOyster => {
                let run = self.run_ctx_mut();
                run.max_hp += 11;
                run.hp = (run.hp + 11).min(run.max_hp);
            }
            // These have no after_obtained effect in the frozen r7 source. Their ongoing relic
            // behaviour belongs to the relic packet and is not simulated by this pickup hook.
            RelicId::BoomingConch
            | RelicId::FishingRod
            | RelicId::LavaRock
            | RelicId::SilverCrucible
            | RelicId::StoneHumidifier
            | RelicId::WingedBoots => {}
            // Nested cross-character rewards and recursive ancient-relic rewards still need
            // dedicated queue semantics. Keep these source-grounded and fail closed.
            RelicId::Kaleidoscope => {
                return self.offer_kaleidoscope();
            }
            RelicId::NeowsBones => {
                return self.offer_neows_bones();
            }
            _ => {
                self.unported = Some("NEOW:unexpected pickup hook");
                return Ok(());
            }
        }

        self.unported = None;
        self.phase = RunPhase::MapChoice;
        Ok(())
    }

    pub fn take_action(&mut self, action: RunAction) -> Result<(), &'static str> {
        self.native_decision_started = true;
        if matches!(self.native_event_id(), Some(EventId::StoneOfAllTime | EventId::RanwidTheElder))
            && matches!(action.kind, RunActionKind::UsePotion | RunActionKind::DiscardPotion)
        {
            return Err("event locks potion use and removal");
        }
        // docs/mp_run_layer.md: this manager owns only seat zero's persistent
        // deck/inventory. RunAction.player_id uses network IDs (seat zero = 1).
        // Reject nonlocal acquisition and its continuations BEFORE mutating the
        // solo run, regardless of which relic an event/reward eventually picks.
        // In particular PrecariousShears, PaelsGrowth, Whetstone, PandorasBox,
        // NeowsTalisman and PotionBelt must never act on seat zero for a teammate.
        if (self.phase != RunPhase::Combat || action.kind == RunActionKind::DiscardPotion)
            && action.player_id.is_some_and(|id| id != PLAYER_ID)
        {
            self.unported = Some("owner pickup needs MP run layer");
            return Err("owner pickup needs MP run layer");
        }
        // sim_r1_events: same precedence as r7 RunManager.take_action.
        if self.phase != RunPhase::Combat && self.pending_run_choice.is_some() {
            return match action.kind {
                RunActionKind::Choose => {
                    self.resolve_run_choice(action.index.ok_or("missing choice index")?)
                }
                RunActionKind::ConfirmChoice => self.confirm_pending_run_choice(),
                RunActionKind::PickCard if self.pending_run_choice.as_ref()
                    .is_some_and(|c| c.single_select && !c.reward_cards.is_empty()) =>
                    self.resolve_run_choice(action.index.ok_or("missing card index")?),
                RunActionKind::UsePotion => self.use_potion_out_of_combat(action),
                RunActionKind::DiscardPotion => {
                    let slot = action.slot_index.ok_or("missing potion slot")?;
                    let run = self.run_ctx_mut();
                    if slot >= run.potions.len() || run.potions[slot].is_none() {
                        return Err("invalid potion slot");
                    }
                    run.potions[slot] = None;
                    Ok(())
                }
                _ => Err("pending run choice must be resolved first"),
            };
        }
        match action.kind {
            RunActionKind::EventChoice => {
                let typed_option = if let Some(option) = action.event_option_id {
                    option
                } else if self.current_act == 0
                    && self.run_ctx().total_floor == 1
                    && self.current_event.is_none()
                {
                    let wire = action.option_id.ok_or("missing event option")?;
                    self.neow.iter().find(|choice| choice.option_id.legacy_id() == wire)
                        .map(|choice| choice.option_id).ok_or("invalid Neow option")?
                } else {
                    let wire = action.option_id.ok_or("missing event option")?;
                    self.current_event.as_ref().and_then(|event| event.options.iter()
                        .find(|option| option.option_id.legacy_id() == wire).map(|option| option.option_id))
                        .ok_or("invalid event option")?
                };
                if self.current_act == 0
                    && self.run_ctx().total_floor == 1
                    && self.current_event.is_none()
                {
                    return self.resolve_neow_choice(typed_option);
                }
                self.take_event_choice(typed_option)
            }
            RunActionKind::DiscardPotion => {
                let slot = action.slot_index.ok_or("missing potion slot")?;
                let run = self.run_ctx_mut();
                let id = run
                    .potions
                    .get(slot)
                    .copied()
                    .flatten()
                    .ok_or("invalid potion slot")?;
                if action.potion_id != Some(id) {
                    return Err("potion id mismatch");
                }
                run.potions[slot] = None;
                Ok(())
            }
            RunActionKind::UsePotion if self.phase != RunPhase::Combat => {
                self.use_potion_out_of_combat(action)
            }
            RunActionKind::Move => self.move_to(action.coord.ok_or("missing coord")?),
            RunActionKind::LeaveShop if self.phase == RunPhase::Shop => {
                self.run_choice = None;
                self.phase = RunPhase::MapChoice;
                Ok(())
            }
            RunActionKind::BuyCard if self.phase == RunPhase::Shop => {
                self.buy_shop_card(action.index.ok_or("missing shop card index")?)
            }
            RunActionKind::BuyRelic if self.phase == RunPhase::Shop => {
                self.buy_shop_relic(action.index.ok_or("missing shop relic index")?)
            }
            RunActionKind::BuyPotion if self.phase == RunPhase::Shop => {
                self.buy_shop_potion(action.index.ok_or("missing shop potion index")?)
            }
            RunActionKind::RemoveCard if self.phase == RunPhase::Shop => self.begin_shop_removal(),
            RunActionKind::RestOption if self.phase == RunPhase::RestSite => {
                let kind = if let Some(kind) = action.rest_option {
                    kind
                } else {
                    let wire = action.option_id.ok_or("missing rest option id")?;
                    self.rest.options.iter().find(|option| option.kind.legacy_id() == wire)
                        .map(|option| option.kind).ok_or("rest option unavailable")?
                };
                self.take_rest_option(kind)
            }
            RunActionKind::Choose if matches!(self.phase, RunPhase::Shop | RunPhase::RestSite) => {
                self.take_run_choice(action.index.ok_or("missing choice index")?)
            }
            RunActionKind::ConfirmChoice if self.phase == RunPhase::RestSite => {
                self.confirm_run_choice()
            }
            RunActionKind::EndTurn if self.phase == RunPhase::Combat => {
                self.combat.as_mut().ok_or("no combat")?.end_player_turn();
                self.after_combat_action()
            }
            RunActionKind::PlayCard if self.phase == RunPhase::Combat => {
                let ok = self.combat.as_mut().ok_or("no combat")?.play_card(
                    crate::combat::PLAYER,
                    action.hand_index.ok_or("missing hand index")?,
                    action.target_index,
                );
                if !ok {
                    return Err("play_card refused");
                }
                self.after_combat_action()
            }
            RunActionKind::UsePotion if self.phase == RunPhase::Combat => {
                let ok = self.combat.as_mut().ok_or("no combat")?.use_potion(
                    action.slot_index.ok_or("missing potion slot")?,
                    action.target_index,
                );
                if !ok {
                    return Err("use_potion refused");
                }
                self.after_combat_action()
            }
            RunActionKind::Choose if self.phase == RunPhase::Combat => {
                let ok = self
                    .combat
                    .as_mut()
                    .ok_or("no combat")?
                    .resolve_pending_choice(action.index);
                if !ok {
                    return Err("choice refused");
                }
                self.after_combat_action()
            }
            RunActionKind::ConfirmChoice if self.phase == RunPhase::Combat => {
                let ok = self
                    .combat
                    .as_mut()
                    .ok_or("no combat")?
                    .resolve_pending_choice(None);
                if !ok {
                    return Err("confirm refused");
                }
                self.after_combat_action()
            }
            RunActionKind::PickCard if self.phase == RunPhase::CardReward => {
                self.pick_card(action.index.ok_or("missing card index")?)
            }
            RunActionKind::RerollCardReward if self.phase == RunPhase::CardReward => {
                self.reroll_card_reward()
            }
            RunActionKind::SacrificeCardReward if self.phase == RunPhase::CardReward => {
                self.sacrifice_card_reward()
            }
            RunActionKind::PickCardBundle if self.phase == RunPhase::CardReward => {
                self.pick_card_bundle(action.index.ok_or("missing bundle index")?)
            }
            RunActionKind::Skip if self.phase == RunPhase::CardReward => self.skip_card_reward(),
            RunActionKind::SkipRewards if self.phase == RunPhase::CardReward => {
                self.skip_reward_set()
            }
            RunActionKind::SkipPotion if self.phase == RunPhase::CardReward => {
                self.skip_active_reward(false)
            }
            RunActionKind::PickPotion if self.phase == RunPhase::CardReward => {
                self.pick_potion(action.reward_index)
            }
            RunActionKind::SkipRelic if self.phase == RunPhase::CardReward => {
                self.skip_active_reward(true)
            }
            RunActionKind::PickRelicReward if self.phase == RunPhase::CardReward => {
                self.pick_relic_reward(action.reward_index)
            }
            RunActionKind::OpenReward if self.phase == RunPhase::CardReward => {
                self.open_reward(action.reward_index.ok_or("missing reward index")?)
            }
            RunActionKind::PickSpecialCard if self.phase == RunPhase::CardReward => {
                self.pick_special_card(action.reward_index.ok_or("missing reward index")?)
            }
            RunActionKind::Collect if self.phase == RunPhase::Treasure => self.collect_treasure(),
            _ => Err("action not valid in current phase"),
        }
    }

    fn use_potion_out_of_combat(&mut self, action: RunAction) -> Result<(), &'static str> {
        if self.out_of_combat_potions_locked() {
            return Err("potion removal is locked by event");
        }
        let slot = action.slot_index.ok_or("missing potion slot")?;
        let id = self
            .run_ctx()
            .potions
            .get(slot)
            .copied()
            .flatten()
            .ok_or("invalid potion slot")?;
        if action.potion_id != Some(id) {
            return Err("potion id mismatch");
        }
        if POTION_INFO[id.index()].usage != PotionUsageType::AnyTime {
            return Err("potion is combat-only");
        }
        if POTION_INFO[id.index()].target == PotionTargetType::AnyPlayer
            && action.target_player_id != Some(PLAYER_ID)
        {
            return Err("invalid potion target player");
        }
        if id == PotionId::Foulpotion {
            if self.native_event_id() == Some(EventId::FakeMerchant) {
                // FakeMerchant.FoulPotionThrown snapshots only remaining stock;
                // unlike MerchantRoom, FoulPotion.OnUse grants no immediate gold.
                let event = self.current_event.take().unwrap();
                self.fake_merchant_loot = vec![RelicId::FakeMerchantsRug];
                for (i, &id) in event.relics.iter().enumerate() {
                    if event.flags & (1 << i) == 0 { self.fake_merchant_loot.push(id); }
                }
                self.run_ctx_mut().potions[slot] = None;
                self.unported = None;
                return self.enter_encounter(RoomType::Monster, EncounterId::FakeMerchantEventEncounter);
            }
            if self.phase != RunPhase::Shop {
                return Err("Foul Potion requires a merchant");
            }
            self.run_ctx_mut().potions[slot] = None;
            treasure::gain_gold(self.run_ctx_mut(), 100);
            return Ok(());
        }
        // Frozen r7 removes the potion before applying its any-time effect.
        self.run_ctx_mut().potions[slot] = None;
        // run: potions/all.py + run_manager.py:_do_use_potion_out_of_combat
        match id {
            PotionId::Ambergris => {
                let run = self.run_ctx_mut();
                run.hp = (run.hp + run.max_hp * 50 / 100).min(run.max_hp);
            }
            PotionId::Bloodpotion => {
                let run = self.run_ctx_mut();
                run.hp = (run.hp + run.max_hp * 20 / 100).min(run.max_hp);
            }
            PotionId::Fruitjuice => {
                let run = self.run_ctx_mut();
                run.max_hp += 5;
                run.hp += 5;
            }
            PotionId::Entropicbrew => loop {
                let has_room = {
                    let run = self.run_ctx();
                    run.potions.iter().any(Option::is_none)
                        || run.potions.len() < run.max_potion_slots as usize
                };
                if !has_room {
                    break;
                }
                let Some(potion) = shop::random_potion(self.run_ctx_mut()) else {
                    break;
                };
                if !shop::procure_potion(self.run_ctx_mut(), potion) {
                    break;
                }
            },
            _ => {
                self.unported = Some("out-of-combat potion effect");
                return Err("out-of-combat potion effect unported");
            }
        }
        Ok(())
    }

    fn out_of_combat_potions_locked(&self) -> bool {
        // PunchOff.TakeThem only changes the page; AnyTime potions remain
        // executable and discardable before entering its retained combat.
        matches!(self.native_event_id(),Some(EventId::TheFutureOfPotions|EventId::StoneOfAllTime|EventId::RanwidTheElder))
    }

    fn buy_shop_card(&mut self, index: usize) -> Result<(), &'static str> {
        let entry = self
            .shop
            .cards
            .iter()
            .chain(self.shop.colorless_cards.iter())
            .nth(index)
            .ok_or("invalid shop card index")?;
        let price = entry.price;
        let mut card = entry.card.clone().ok_or("shop card sold out")?;
        if self.run_ctx().gold < price {
            return Err("cannot afford shop card");
        }
        {
            let run = self.run_ctx_mut();
            run.gold -= price;
            card.instance_id = run.new_card_id();
            run.deck.push(card);
            // MerchantCardEntry.cs:130: CardPileCmd.Add(Deck) fires entry hooks.
            run.after_run_deck_entry(run.deck.len() - 1);
        }
        let refill = shop::post_purchase(self.run_ctx_mut(), price);
        if refill {
            let rarity_offset = self.card_rarity_offset as f64;
            let run = if let Some(combat) = self.combat.as_mut() {
                &mut combat.run
            } else {
                self.run.as_mut().expect("run context")
            };
            shop::refill_card(&mut self.shop, index, run, rarity_offset)?;
        } else if index < self.shop.cards.len() {
            self.shop.cards[index].card = None;
            self.shop.cards[index].price = shop::SOLD_OUT;
        } else {
            let i = index - self.shop.cards.len();
            let entry = self
                .shop
                .colorless_cards
                .get_mut(i)
                .ok_or("invalid shop card index")?;
            entry.card = None;
            entry.price = shop::SOLD_OUT;
        }
        Ok(())
    }

    fn buy_shop_relic(&mut self, index: usize) -> Result<(), &'static str> {
        let entry = *self
            .shop
            .relics
            .get(index)
            .ok_or("invalid shop relic index")?;
        if entry.price >= shop::SOLD_OUT {
            return Err("shop relic sold out");
        }
        if self.run_ctx().gold < entry.price {
            return Err("cannot afford shop relic");
        }
        self.remove_relic_from_bags(entry.relic_id)?;
        self.run_ctx_mut().gold -= entry.price;
        let pickup = shop::add_relic(self.run_ctx_mut(), entry.relic_id);
        let refill = shop::post_purchase(self.run_ctx_mut(), entry.price);
        if refill {
            let run = if let Some(combat) = self.combat.as_mut() {
                &mut combat.run
            } else {
                self.run.as_mut().expect("run context")
            };
            shop::refill_relic(&mut self.shop, index, run, &mut self.relic_bags)?;
        } else {
            self.shop.relics[index].price = shop::SOLD_OUT;
        }
        if self.offer_nr_pickup(entry.relic_id)? {
            return Ok(());
        }
        if let Err(reason) = pickup {
            if entry.relic_id == RelicId::Orrery {
                // Orrery.cs AfterObtained: five independent 3-option CardRewards,
                // owner character pool, Other source, RegularEncounter rarity odds.
                self.reward = RewardState::default();
                self.reward.stay_in_container = true;
                for _ in 0..5 {
                    let reward = self.make_noncombat_card_reward(3, self.character.into(), None, true, true, false)?;
                    self.reward.items.push(reward);
                    if self.unported.is_some() { return Ok(()); }
                }
                self.phase = RunPhase::CardReward;
                return Ok(());
            }
            self.unported = Some(reason);
        }
        Ok(())
    }

    fn buy_shop_potion(&mut self, index: usize) -> Result<(), &'static str> {
        let entry = *self
            .shop
            .potions
            .get(index)
            .ok_or("invalid shop potion index")?;
        if entry.price >= shop::SOLD_OUT {
            return Err("shop potion sold out");
        }
        if self.run_ctx().gold < entry.price {
            return Err("cannot afford shop potion");
        }
        // MerchantPotionEntry.OnTryPurchase first awaits TryToProcure.
        // FailureSpace/FailureForbidden returns before gold, stock, refill,
        // and AfterItemPurchased hooks; a direct failed attempt is a no-op.
        if !shop::procure_potion(self.run_ctx_mut(), entry.potion_id) {
            return Ok(());
        }
        self.run_ctx_mut().gold -= entry.price;
        let refill = shop::post_purchase(self.run_ctx_mut(), entry.price);
        if refill {
            let run = if let Some(combat) = self.combat.as_mut() {
                &mut combat.run
            } else {
                self.run.as_mut().expect("run context")
            };
            shop::refill_potion(&mut self.shop, index, run)?;
        } else {
            self.shop.potions[index].price = shop::SOLD_OUT;
        }
        Ok(())
    }

    fn begin_shop_removal(&mut self) -> Result<(), &'static str> {
        if self.shop.removal_used || self.run_choice.is_some() {
            return Err("shop removal unavailable");
        }
        let price = self.shop.removal_cost;
        if self.run_ctx().gold < price {
            return Err("cannot afford card removal");
        }
        let candidates = rest::removable_indices(self.run_ctx());
        if candidates.is_empty() {
            return Err("no removable cards");
        }
        self.run_ctx_mut().gold -= price;
        self.run_choice = Some(RunDeckChoice {
            kind: RunDeckChoiceKind::ShopRemove { gold_spent: price },
            candidates,
            selected: Vec::new(),
        });
        Ok(())
    }

    fn finish_rest_option(&mut self, index: usize) {
        let keep_others = rest::miniature_tent(self.run_ctx());
        rest::disable_after_choice(&mut self.rest, index, keep_others);
        self.run_choice = None;
        if keep_others && self.rest.options.iter().any(|option| option.enabled) {
            self.phase = RunPhase::RestSite;
        } else {
            self.phase = RunPhase::MapChoice;
        }
    }

    fn prepare_rest_reward(&mut self, index: usize) {
        let keep_others = rest::miniature_tent(self.run_ctx());
        rest::disable_after_choice(&mut self.rest, index, keep_others);
        self.reward_returns_to_rest =
            keep_others && self.rest.options.iter().any(|option| option.enabled);
        self.run_choice = None;
    }

    fn take_rest_option(&mut self, kind: rest::RestOptionKind) -> Result<(), &'static str> {
        let index = rest::option_index(&self.rest, kind).ok_or("rest option unavailable")?;
        let kind = self.rest.options[index].kind;
        match kind {
            rest::RestOptionKind::Mend => return Err("Mend requires multiplayer run layer"),
            rest::RestOptionKind::Heal => {
                let _ = rest::execute_heal(self.run_ctx_mut());
                if rest::has_heal_followups(self.run_ctx()) {
                    self.prepare_rest_reward(index);
                    self.generate_rest_heal_rewards();
                } else {
                    self.finish_rest_option(index);
                }
            }
            rest::RestOptionKind::Smith => {
                let candidates = rest::upgradable_indices(self.run_ctx_mut());
                if candidates.is_empty() {
                    self.finish_rest_option(index);
                } else {
                    self.run_choice = Some(RunDeckChoice {
                        kind: RunDeckChoiceKind::Smith { rest_option: index },
                        candidates,
                        selected: Vec::new(),
                    });
                }
            }
            rest::RestOptionKind::Dig => {
                let relic = self.pull_player_relic(None, false);
                self.finish_rest_option(index);
                self.obtain_event_relic(relic)?;
            }
            rest::RestOptionKind::Lift => {
                if !rest::execute_lift(self.run_ctx_mut()) {
                    return Err("Girya cannot lift");
                }
                self.finish_rest_option(index);
            }
            rest::RestOptionKind::Cook => {
                let candidates = rest::removable_indices(self.run_ctx());
                if candidates.len() < 2 {
                    return Err("not enough removable cards to cook");
                }
                self.run_choice = Some(RunDeckChoice {
                    kind: RunDeckChoiceKind::Cook { rest_option: index },
                    candidates,
                    selected: Vec::new(),
                });
            }
            rest::RestOptionKind::Clone => {
                let _ = rest::execute_clone(self.run_ctx_mut());
                self.finish_rest_option(index);
            }
            rest::RestOptionKind::Kindle => {
                if !rest::execute_kindle(self.run_ctx_mut()) {
                    return Err("Pumpkin Candle missing");
                }
                self.finish_rest_option(index);
            }
            rest::RestOptionKind::Hatch => {
                self.remove_relic_from_bags(RelicId::Byrdpip)?;
                // C# Byrdpip transforms every deck egg before rest completion.
                shop::attach_relic(self.run_ctx_mut(),RelicId::Byrdpip);
                self.apply_byrdpip_pickup()?;
                self.finish_rest_option(index);
            }
        }
        Ok(())
    }

    fn generate_rest_heal_rewards(&mut self) {
        // Frozen r7 builds these pending rewards in relic order, and populates
        // them in that order before sorting the visible reward container.
        let relics: Vec<RelicId> = self.run_ctx().owned_relics(crate::combat::PLAYER).map(|r| r.id).collect();
        self.reward = RewardState::default();
        for relic in relics {
            match relic {
                RelicId::TinyMailbox => {
                    for _ in 0..2 {
                        let Some(potion) = self.roll_potion() else { return; };
                        self.reward.items.push(RewardItem {
                            kind: RewardItemKind::Potion(potion),
                            consumed: false,
                            skippable: true,
                            skip_completes: true,
                            add_injury_after: false,
                            neows_bones_final: false,
                        });
                    }
                }
                RelicId::DreamCatcher => {
                    let mut chosen = ArrayVec::<CardId, 3>::new();
                    let mut cards = ArrayVec::<Card, 4>::new();
                    for _ in 0..3 {
                        let rarity = self.roll_card_rarity(RoomType::RestSite);
                        let Some(card) = self.generate_reward_card(rarity, &chosen) else {
                            return;
                        };
                        chosen.push(card.id);
                        cards.push(card);
                    }
                    self.apply_card_reward_relic_hooks(&mut cards, true);
                    self.reward.items.push(RewardItem {
                        kind: RewardItemKind::Card {
                            cards,
                            closed: false,
                            // C# DreamCatcher.cs:22 uses ForRoom(Monster), not RestSite.
                            reroll: Some(CardRewardRerollOptions::Encounter(RoomType::Monster)),
                            progress: Default::default(),
                        },
                        consumed: false,
                        skippable: true,
                        skip_completes: true,
                        add_injury_after: false,
                        neows_bones_final: false,
                    });
                }
                _ => {}
            }
        }
        self.reward.active = None;
        self.advance_rewards();
    }

    fn take_run_choice(&mut self, option_index: usize) -> Result<(), &'static str> {
        let choice = self.run_choice.as_ref().ok_or("no run choice")?;
        let deck_index = *choice
            .candidates
            .get(option_index)
            .ok_or("invalid run choice index")?;
        let kind = choice.kind;
        match kind {
            RunDeckChoiceKind::Smith { rest_option } => {
                let _ = self.run_ctx_mut().upgrade_deck_card(deck_index);
                self.finish_rest_option(rest_option);
            }
            RunDeckChoiceKind::ShopRemove { gold_spent } => {
                if deck_index >= self.run_ctx().deck.len() {
                    return Err("invalid removal card");
                }
                self.run_ctx_mut().remove_run_deck_card(deck_index);
                self.run_choice = None;
                self.shop.removal_used = true;
                self.shop_removals_used += 1;
                let _ = shop::post_purchase(self.run_ctx_mut(), gold_spent);
            }
            RunDeckChoiceKind::Cook { .. } => {
                let choice = self.run_choice.as_mut().expect("run choice");
                if let Some(position) = choice.selected.iter().position(|x| *x == deck_index) {
                    choice.selected.remove(position);
                } else if choice.selected.len() < 2 {
                    choice.selected.push(deck_index);
                }
            }
        }
        Ok(())
    }

    fn confirm_run_choice(&mut self) -> Result<(), &'static str> {
        let choice = self.run_choice.as_ref().ok_or("no run choice")?;
        let RunDeckChoiceKind::Cook { rest_option } = choice.kind else {
            return Err("choice cannot be confirmed");
        };
        if choice.selected.len() != 2 {
            return Err("cook requires two cards");
        }
        let mut selected = choice.selected.clone();
        selected.sort_unstable_by(|a, b| b.cmp(a));
        for deck_index in selected {
            if deck_index >= self.run_ctx().deck.len() {
                return Err("invalid cooked card");
            }
            self.run_ctx_mut().remove_run_deck_card(deck_index);
        }
        self.run_ctx_mut().max_hp += 9;
        self.run_ctx_mut().hp += 9;
        self.finish_rest_option(rest_option);
        Ok(())
    }

    // ---- sim_r1_events: run-level deck-choice plumbing ---------------------

    fn begin_run_card_choice(
        &mut self,
        source: &'static str,
        prompt: &'static str,
        candidate_indices: &[usize],
        min_choices: usize,
        max_choices: usize,
        allow_skip: bool,
        effect: RunChoiceEffect,
        resume: ChoiceResume,
    ) {
        assert!(!source.is_empty(), "selector site must have a stable name");
        let run = self.run_ctx();
        let options = candidate_indices
            .iter()
            .filter_map(|&i| run.deck.get(i))
            .map(|card| RunChoiceCard {
                instance_id: card.instance_id,
                card_id: card.id,
                source_pile: "deck",
            })
            .collect();
        self.pending_run_choice = Some(RunCardChoice {
            wire_source: Some(source),
            prompt,
            options,
            selected: Vec::new(),
            min_choices,
            max_choices,
            allow_skip,
            single_select: min_choices == 1 && max_choices == 1,
            effect,
            resume,
            reward_cards: Vec::new(),
        });
        self.phase = RunPhase::CardReward;
    }

    fn upgradable_deck_indices(&mut self) -> Vec<usize> {
        let len = self.run_ctx().deck.len();
        let mut out = Vec::new();
        for i in 0..len {
            if self.run_ctx_mut().is_deck_card_upgradable(i) {
                out.push(i);
            }
        }
        out
    }

    fn removable_deck_indices(&self) -> Vec<usize> {
        Self::removable_card_indices(&self.run_ctx().deck)
    }

    pub(super) fn removable_card_indices(cards: &[Card]) -> Vec<usize> {
        let mut curses = Vec::new();
        let mut others = Vec::new();
        for (i, card) in cards.iter().enumerate() {
            // r7 CardInstance.is_removable: Ascender's Bane and quest cards are fixed.
            if card.id == CardId::AscendersBane || card.card_type == CardType::Quest
                || card.has_kw(KW_ETERNAL) {
                continue;
            }
            if card.card_type == CardType::Curse {
                curses.push(i);
            } else {
                others.push(i);
            }
        }
        curses.extend(others);
        curses
    }

    fn transformable_deck_indices(&self) -> Vec<usize> {
        self.run_ctx()
            .deck
            .iter()
            .enumerate()
            .filter_map(|(i, card)| {
                (card.id != CardId::AscendersBane && card.card_type != CardType::Quest
                    && !card.has_kw(KW_ETERNAL)).then_some(i)
            })
            .collect()
    }

    fn resolve_run_choice(&mut self, index: usize) -> Result<(), &'static str> {
        let Some(choice) = self.pending_run_choice.as_mut() else {
            return Err("no pending run choice");
        };
        if choice.is_multi() {
            if !choice.toggle(index) {
                return Err("choice refused");
            }
            // CardSelectorPrefs defaults to automatic completion for fixed
            // counts. RewardSynchronizer explicitly requests manual removal.
            if choice.min_choices == choice.max_choices
                && !matches!(choice.effect, RunChoiceEffect::RemoveReward { .. })
                && choice.selected.len() == choice.max_choices
            {
                let selected = choice.selected.clone();
                let choice = self.pending_run_choice.take().unwrap();
                return self.apply_run_choice(choice, &selected);
            }
            return Ok(());
        }
        if index >= choice.options.len() {
            return Err("choice index out of range");
        }
        let choice = self.pending_run_choice.take().unwrap();
        self.apply_run_choice(choice, &[index])
    }

    fn confirm_pending_run_choice(&mut self) -> Result<(), &'static str> {
        let Some(choice) = self.pending_run_choice.as_ref() else {
            return Err("no pending run choice");
        };
        if choice.is_multi() {
            if !choice.can_confirm() {
                return Err("confirm refused");
            }
            let selected = choice.selected.clone();
            let choice = self.pending_run_choice.take().unwrap();
            return self.apply_run_choice(choice, &selected);
        }
        if !choice.allow_skip {
            return Err("confirm refused");
        }
        let choice = self.pending_run_choice.take().unwrap();
        self.apply_run_choice(choice, &[])
    }

    fn apply_run_choice(
        &mut self,
        choice: RunCardChoice,
        selected_indices: &[usize],
    ) -> Result<(), &'static str> {
        // C# MorphicGrove.cs:47-50 / Astrolabe.cs:20-28 iterate the
        // selector's returned sequence. Membership-filtering options silently
        // reordered it and assigned RNG draws to the wrong original cards.
        // YummyCookie, TriBoomerang and WaterloggedScriptorium use the same
        // returned-order foreach. PaelsTooth's explicit stable Id.Entry sort
        // remains in paels_tooth_store_selected, not in this shared path.
        let selected_ids: Vec<u32> = selected_indices.iter()
            .map(|&index| choice.options[index].instance_id).collect();
        match choice.effect {
            RunChoiceEffect::StorePaelsTooth => {
                // Preserve the selector's returned order for stable equal-entry ties.
                let indices: Vec<_> = selected_indices.iter().filter_map(|&i| {
                    let id = choice.options.get(i)?.instance_id;
                    self.run_ctx().deck.iter().position(|c| c.instance_id == id)
                }).collect();
                self.run_ctx_mut().paels_tooth_store_selected(&indices);
            }
            RunChoiceEffect::RemoveReward { owner, reward_index } => {
                // RewardSynchronizer.DoUnsyncedCardRemoval: cancellation keeps
                // the reward; only a confirmed selection consumes it.
                if !selected_ids.is_empty() {
                    if owner == crate::combat::PLAYER {
                        self.run_ctx_mut().retain_run_deck_cards(|c| !selected_ids.contains(&c.instance_id));
                    } else {
                        self.combat.as_mut().ok_or("reward owner combat missing")?
                            .retain_player_deck_cards(owner,|c| !selected_ids.contains(&c.instance_id));
                    }
                    self.reward.items[reward_index].consumed = true;
                    self.reward.active = None;
                    self.advance_rewards();
                } else {
                    self.reward.active = None;
                    self.phase = RunPhase::CardReward;
                }
                return Ok(());
            }
            RunChoiceEffect::AddCards => {
                // C# CardSelectCmd returns the selected index sequence; EventModel
                // appends cards in that order, not their original grid order.
                for &index in selected_indices {
                    let card = choice.reward_cards.get(index).cloned()
                        .ok_or("invalid reward card selection")?;
                    self.add_card_instance_to_deck(card)?;
                }
            }
            RunChoiceEffect::Duplicate => {
                // DollysMirror.AfterObtained -> RunState.CloneCard -> Deck Add.
                // Preserve the selected instance's mutable card state, allocate
                // one fresh identity, then run ordinary deck-entry hooks.
                for instance_id in selected_ids {
                    let original=self.run_ctx().deck.iter()
                        .find(|c|c.instance_id==instance_id).cloned()
                        .ok_or("Dolly's Mirror selected card disappeared")?;
                    let iid=self.run_ctx_mut().new_card_id();
                    self.add_card_instance_to_deck(original.clone_with_id(iid))?;
                }
            }
            RunChoiceEffect::Upgrade => {
                for instance_id in selected_ids {
                    let Some(index) = self
                        .run_ctx()
                        .deck
                        .iter()
                        .position(|card| card.instance_id == instance_id)
                    else {
                        continue;
                    };
                    self.run_ctx_mut().upgrade_deck_card(index);
                }
            }
            RunChoiceEffect::UpgradeAndDamage(amount) => {
                for instance_id in selected_ids {
                    let Some(index) = self
                        .run_ctx()
                        .deck
                        .iter()
                        .position(|card| card.instance_id == instance_id)
                    else {
                        continue;
                    };
                    self.run_ctx_mut().upgrade_deck_card(index);
                }
                self.apply_event_effect(EventEffect::LoseHp(amount))?;
                if self.run_ctx().hp == 0 || self.unported.is_some() {
                    self.phase = if self.run_ctx().hp == 0 {
                        RunPhase::RunOver
                    } else {
                        RunPhase::CardReward
                    };
                    return Ok(());
                }
            }
            RunChoiceEffect::Remove { damage_after } => {
                if !selected_ids.is_empty() {
                    self.run_ctx_mut()
                        .deck
                        .retain(|card| !selected_ids.contains(&card.instance_id));
                }
                if damage_after > 0 {
                    // C# PrecariousShears.AfterObtained uses CreatureCmd.Damage.
                    self.apply_event_effect(EventEffect::LoseHp(damage_after))?;
                    if self.run_ctx().hp == 0 {
                        return Ok(());
                    }
                }
            }
            RunChoiceEffect::RemoveAndLoseGold(gold) => {
                if !selected_ids.is_empty() {
                    self.run_ctx_mut()
                        .deck
                        .retain(|card| !selected_ids.contains(&card.instance_id));
                }
                let run = self.run_ctx_mut();
                run.gold = (run.gold - gold.max(0)).max(0);
            }
            RunChoiceEffect::RemoveAndAddCard(card_id) => {
                if !selected_ids.is_empty() {
                    self.run_ctx_mut()
                        .deck
                        .retain(|card| !selected_ids.contains(&card.instance_id));
                }
                self.add_card_to_deck(card_id)?;
            }
            RunChoiceEffect::Transform => {
                for instance_id in selected_ids {
                    self.transform_deck_card(instance_id)?;
                }
            }
            RunChoiceEffect::TransformMaul=>{
                for instance in selected_ids {
                    let original=self.run_ctx().deck.iter().find(|c|c.instance_id==instance).cloned()
                        .ok_or("Claws original missing")?;
                    let id=self.run_ctx_mut().new_card_id();
                    let replacement=super::tanx::maul(&original,id)?;
                    let index=self.run_ctx().deck.iter().position(|c|c.instance_id==instance).unwrap();
                    self.run_ctx_mut().retain_run_deck_cards(|c|c.instance_id!=instance);
                    let added=self.run_ctx().deck.len();
                    self.add_card_instance_to_deck(replacement)?;
                    let replacement=self.run_ctx_mut().remove_run_deck_card(added);
                    self.run_ctx_mut().deck.insert(index,replacement);
                }
            }
            RunChoiceEffect::TransformUpgrade => self.apply_astrolabe_choice(&selected_ids)?,
            RunChoiceEffect::TransformEventRng | RunChoiceEffect::TransformEventRngAndDamage(_) => {
                let mut event = self.current_event.take().ok_or("event transform missing state")?;
                for instance_id in selected_ids {
                    self.transform_deck_card_with_rng(instance_id, &mut event.rng)?;
                }
                self.current_event = Some(event);
                if let RunChoiceEffect::TransformEventRngAndDamage(amount) = choice.effect {
                    self.apply_event_effect(EventEffect::LoseHp(amount))?;
                }
            }
            RunChoiceEffect::Enchant { name, amount } => {
                for instance_id in selected_ids {
                    let Some(card) = self
                        .run_ctx_mut()
                        .deck
                        .iter_mut()
                        .find(|card| card.instance_id == instance_id)
                    else {
                        continue;
                    };
                    let next_amount = if let Some((_, value)) = card
                        .enchantments
                        .iter_mut()
                        .find(|(existing, _)| *existing == name)
                    {
                        *value += amount;
                        *value
                    } else {
                        amount
                    };
                    if !crate::card::enchant::apply_static_enchantment(card, name, next_amount) {
                        return Err("event enchantment capacity exceeded");
                    }
                }
            }
        }
        // C# StoneOfAllTime.Push draws only AFTER the enchant selection returns.
        if self.native_event_id() == Some(EventId::StoneOfAllTime) {
            self.current_event.as_mut().unwrap().rng.next_int_exclusive(0, 100);
        }
        self.phase = if self.run_ctx().hp <= 0 { RunPhase::RunOver } else { match choice.resume {
            ChoiceResume::MapChoice => RunPhase::MapChoice,
            ChoiceResume::Event => RunPhase::Event,
            ChoiceResume::Shop => RunPhase::Shop,
        }};
        if choice.resume == ChoiceResume::MapChoice {
            if !matches!(self.native_event_id(), Some(EventId::RanwidTheElder | EventId::WelcomeToWongos)) {
                if let Some(event)=&self.current_event {self.replay_event=Some(event.clone());}
                self.current_event = None;
            }
            if !self.reward_parents.is_empty() && self.phase != RunPhase::RunOver {
                self.after_rewards();
            } else if self.native_event_id() == Some(EventId::RanwidTheElder)
                && self.phase != RunPhase::RunOver {
                self.resume_ranwid_pickups()?;
            } else if self.native_event_id() == Some(EventId::WelcomeToWongos)
                && self.phase != RunPhase::RunOver {
                self.finish_wongo_purchase()?;
            }
        }
        if choice.resume == ChoiceResume::Event && self.phase == RunPhase::Event {
            self.resume_conveyor();
        }
        Ok(())
    }

    fn transform_deck_card(&mut self, instance_id: u32) -> Result<(), &'static str> {
        self.transform_deck_card_with_stream(instance_id, false)
    }

    fn transform_deck_card_with_rng(
        &mut self,
        instance_id: u32,
        rng: &mut Rng,
    ) -> Result<(), &'static str> {
        self.transform_deck_card_with_rng_upgrade(instance_id, rng, None)
    }

    /// CardCmd.Transform: create every replacement before removal and deck
    /// entry callbacks, then .NET-sort by the captured removal-time pile index.
    fn transform_deck_cards_batch(&mut self,ids:&[u32])->Result<(),&'static str>{
        let mut transformations=Vec::new();
        for &id in ids{
            let original=self.run_ctx().deck.iter().find(|c|c.instance_id==id).ok_or("transform original missing")?;
            let pool=super::card_pools::transformations(original,self.is_multiplayer())?;
            let owner=original.owner;
            let id_new=pool[self.run_ctx_mut().rngs.niche.choice_index(pool.len())];
            let iid=self.run_ctx_mut().new_card_id();
            let mut card=Card::from_factory(id_new,false,iid).ok_or("transform factory")?;card.owner=owner;
            transformations.push((id,card,0usize));
        }
        for (id,_,index) in &mut transformations{
            *index=self.run_ctx().deck.iter().position(|c|c.instance_id==*id).ok_or("transform original missing")?;
            self.run_ctx_mut().deck.remove(*index);
        }
        let mut order:Vec<crate::card::KRef>=(0..transformations.len()).map(|i|i as crate::card::KRef).collect();
        crate::flow::dotnet_sort_indices(&mut order,|a,b|transformations[a as usize].2<transformations[b as usize].2);
        for i in order{self.add_card_instance_to_deck(transformations[i as usize].1.clone())?;}
        Ok(())
    }

    fn transform_deck_card_with_rng_upgrade(
        &mut self,
        instance_id: u32,
        rng: &mut Rng,
        upgraded_instance_id: Option<u32>,
    ) -> Result<(), &'static str> {
        let Some(index) = self
            .run_ctx()
            .deck
            .iter()
            .position(|card| card.instance_id == instance_id)
        else {
            return Ok(());
        };
        let original = self.run_ctx().deck[index].clone();
        let candidates = super::card_pools::transformations(&original, self.is_multiplayer())?;
        if candidates.is_empty() {
            return Err("empty transform pool");
        }
        let pick = candidates[rng.choice_index(candidates.len())];
        let iid = upgraded_instance_id.unwrap_or_else(|| self.run_ctx_mut().new_card_id());
        let mut replacement = Card::from_factory(pick, false, iid).ok_or("transform card factory")?;
        replacement.owner = original.owner;
        if upgraded_instance_id.is_some() { shop::upgrade_template(self.run_ctx_mut(), &mut replacement); }
        self.run_ctx_mut().remove_run_deck_card(index);
        self.add_card_instance_to_deck(replacement)
    }

    fn transform_deck_card_with_stream(
        &mut self,
        instance_id: u32,
        transformations_stream: bool,
    ) -> Result<(), &'static str> {
        let mut rng = if transformations_stream {
            self.run_ctx().rngs.transformations.clone()
        } else {
            self.run_ctx().rngs.niche.clone()
        };
        let result = self.transform_deck_card_with_rng(instance_id, &mut rng);
        if transformations_stream {
            self.run_ctx_mut().rngs.transformations = rng;
        } else {
            self.run_ctx_mut().rngs.niche = rng;
        }
        result
    }

    #[inline]
    fn winged_boots_free_travel(&self) -> bool {
        self.run_ctx()
            .owned_relics(crate::combat::PLAYER)
            .any(|relic| relic.id == RelicId::WingedBoots && relic.enabled && relic.counter < 3)
    }

    fn resolve_unknown_room(&mut self, child_all_shop: bool) -> RoomType {
        // run: run/odds.py:65
        let unknown_history_count = self
            .map_history
            .iter()
            .filter(|entry| entry.map_point_type == MapPointType::Unknown)
            .count();
        if self.options.number_of_runs == 0 {
            if unknown_history_count < 2 {
                return RoomType::Event;
            }
            if unknown_history_count == 2 {
                return RoomType::Monster;
            }
        }

        // MONSTER, ELITE, TREASURE, SHOP, EVENT. EVENT is the default fallback.
        let mut allowed = [true, true, true, true, true];
        // run: run/run_state.py:1994
        if child_all_shop
            || self
                .map_history
                .last()
                .is_some_and(|entry| entry.room_type == RoomType::Shop)
        {
            allowed[3] = false;
        }

        // run: cards/status.py:1842
        if self.current_act == 2
            && self
                .run_ctx()
                .deck
                .iter()
                .any(|card| card.id == CardId::LanternKey)
        {
            allowed = [false, false, false, false, true];
        }
        // run: relics/common.py:243
        if self
            .run_ctx()
            .has_owned_relic(crate::combat::PLAYER, RelicId::JuzuBracelet)
        {
            allowed[0] = false;
        }
        // GoldenCompass.cs:48-55 only restricts the acquisition act. Preserve
        // the unknown-room roll and odds update even when Event is the sole type.
        let compass = self.run_ctx().owned_relics(crate::combat::PLAYER)
            .find(|r| r.id == RelicId::GoldenCompass).map(|r| (r.flag1, r.counter2));
        if let Some((known, act)) = compass {
            if !known {
                self.unported = Some("GoldenCompass acquisition act missing");
                return RoomType::Event;
            }
            if act == self.current_act as i32 {
                allowed = [false, false, false, false, true];
            }
        }

        let room_types = [
            RoomType::Monster,
            RoomType::Elite,
            RoomType::Treasure,
            RoomType::Shop,
        ];
        let mut result = RoomType::Event;
        if !allowed[4] {
            if let Some((i, _)) = allowed[..4]
                .iter()
                .enumerate()
                .find(|(_, enabled)| **enabled)
            {
                result = room_types[i];
            }
        }

        let roll = self.run_ctx_mut().rngs.unknown_map_point.next_float(1.0);
        let mut cumulative = 0.0;
        for i in 0..4 {
            if !allowed[i] || self.unknown_odds[i] < 0.0 {
                continue;
            }
            cumulative += self.unknown_odds[i];
            if roll <= cumulative {
                result = room_types[i];
                break;
            }
        }
        for i in 0..4 {
            if result == room_types[i] {
                self.unknown_odds[i] = UNKNOWN_BASE[i];
            } else if allowed[i] {
                self.unknown_odds[i] += UNKNOWN_BASE[i];
            }
        }
        result
    }

    // ---- sim_r1_events: source-ordered EVENT entry/dispatch ----------------

    pub(super) fn event_can_enchant(card: &Card, enchantment: &str) -> bool {
        if matches!(
            card.card_type,
            CardType::Status | CardType::Curse | CardType::Quest
        ) || card.is_unplayable()
        {
            return false;
        }
        // C# EnchantmentModel.CanEnchant: these enchantments are not stackable.
        if !card.enchantments.is_empty() {
            return false;
        }
        match enchantment {
            "RoyallyApproved" => matches!(card.card_type,CardType::Attack|CardType::Skill),
            "SoulsPower" => card.has_kw(KW_EXHAUST),
            "Imbued" => card.card_type == CardType::Skill,
            "Slither" => !card.x_cost,
            "Nimble" => card_info(card.id, card.upgraded).is_some_and(|info| info.gains_block),
            "Goopy" => card.has_tag(CardTag::Defend),
            // cards/enchantments.py: ATTACK_ONLY_ENCHANTMENTS
            "Sharp" | "Momentum" | "Corrupted" | "Favored" | "Vigorous" | "Instinct" => {
                card.card_type == CardType::Attack
            }
            // cards/enchantments.py:62-85
            "Spiral" => {
                card.rarity == CardRarity::Basic
                    && (card.has_tag(CardTag::Strike) || card.has_tag(CardTag::Defend))
            }
            _ => true,
        }
    }

    fn event_relic_is_tradable(relic: &Relic) -> bool {
        let info = &RELIC_INFO[relic.id.index()];
        !relic.is_melted
            // RelicModel.IsTradable first excludes IsUsedUp. The other
            // IsUsedUp overrides are already excluded by rarity/pickup below.
            && !(matches!(relic.id, RelicId::MawBank | RelicId::LizardTail) && relic.flag1)
            && !info.upon_pickup
            && !info.spawns_pets
            && !matches!(
                info.rarity,
                RelicRarity::Starter | RelicRarity::Event | RelicRarity::Ancient
            )
    }

    fn has_event_pet(&self) -> bool {
        // C# Player.cs:183-187 includes the unhatched card, not just relics.
        self.run_ctx().owned_relics(crate::combat::PLAYER)
            .any(|r| matches!(r.id, RelicId::Byrdpip | RelicId::PaelsLegion))
            || self.run_ctx().deck.iter().any(|c| c.id == CardId::ByrdonisEgg)
    }

    fn player_bag_relic_allowed(&self, id: RelicId) -> bool {
        if id == RelicId::LastingCandy && self.character == Character::Ironclad
            && self.options.number_of_runs == 0 { return false; }
        Self::relic_allowed_at_floor(id, self.run_ctx().total_floor, false)
    }

    pub(super) fn relic_allowed_at_floor(id: RelicId, total_floor: i32, multiplayer: bool) -> bool {
        // RelicModel.cs:406-410 and the corresponding relic IsAllowed overrides.
        let late_excluded = [
            "AMETHYST_AUBERGINE", "BOOK_OF_FIVE_RINGS", "BOWLER_HAT", "DRAGON_FRUIT",
            "FROZEN_EGG", "GIRYA", "JUZU_BRACELET", "LASTING_CANDY", "LUCKY_FYSH",
            "MEAL_TICKET", "MOLTEN_EGG", "OLD_COIN", "PLANISPHERE", "SHOVEL",
            "TOXIC_EGG", "WHITE_BEAST_STATUE", "WHITE_STAR",
        ];
        total_floor < (if multiplayer { 38 } else { 41 })
            || !late_excluded.iter().any(|name| RelicId::from_name(name) == Some(id))
    }

    fn prune_player_relic_bag(&mut self) {
        let disallowed: Vec<_> = self.relic_bags.player[0].iter().flatten().copied()
            .filter(|id| !self.player_bag_relic_allowed(*id)).collect();
        for bag in &mut self.relic_bags.player[0] {
            bag.retain(|id| !disallowed.contains(id));
        }
    }

    /// Frozen-r7 `EventModel.is_allowed` checks used by the source-ordered event selector.
    /// This runs only at room entry, so the descriptive event IDs do not enter hot combat paths.
    fn event_is_allowed(&self, id: crate::identity::EventId) -> bool {
        let run = self.run_ctx();
        // RunManager.EnterMapPointInternal calls CreateRoom/PullNextEvent
        // BEFORE AppendToMapPointHistory. TotalFloor is the history count;
        // our run counter has already advanced to the destination room.
        let selection_floor=run.total_floor.saturating_sub(1);
        let act = self.current_act;
        let held_potions = run.potions.iter().filter(|p| p.is_some()).count();
        let removable = self.removable_deck_indices();
        let transformable = self.transformable_deck_indices();
        let tradable_relics = run
            .owned_relics(crate::combat::PLAYER)
            .filter(|relic| Self::event_relic_is_tradable(relic))
            .count();
        let can_enchant = |name: &str| {
            run.deck
                .iter()
                .any(|card| Self::event_can_enchant(card, name))
        };

        match id {
            // events/act1.py
            EventId::BrainLeech | EventId::RoomFullOfCheese => act < 2,
            EventId::TheLegendsWereTrue => act == 0 && !run.deck.is_empty() && run.hp >= 10,
            EventId::TeaMaster => act < 2 && run.gold >= 150,

            // events/act2.py
            EventId::CrystalSphere => act > 0 && run.gold >= 100,
            EventId::DollRoom => act == 1,
            EventId::EndlessConveyor => run.gold >= 120,
            EventId::FakeMerchant => {
                act >= 1
                    && (run.gold >= 100
                        || run.potions.iter().any(|p| *p == Some(PotionId::Foulpotion)))
            }
            EventId::FieldOfManSizedHoles => can_enchant("PerfectFit"),
            // This simulator currently constructs single-player runs, so the source's
            // single-player branch is authoritative here.
            EventId::JungleMazeAdventure => true,
            EventId::LuminousChoir => {
                run.gold >= 149
                    && self
                        .relic_bags
                        .player[crate::combat::PLAYER as usize]
                        .iter()
                        .any(|bag| bag.iter().any(|id| self.player_bag_relic_allowed(*id)))
            }
            EventId::MorphicGrove => run.gold >= 100 && transformable.len() >= 2,
            EventId::PotionCourier => act > 0,
            EventId::RanwidTheElder => {
                act > 0 && run.gold >= 100 && held_potions >= 1 && tradable_relics >= 1
            }
            EventId::RelicTrader => act > 0 && tradable_relics >= 5,
            EventId::SlipperyBridge => selection_floor > 6 && !removable.is_empty(),
            EventId::SpiralingWhirlpool => can_enchant("Spiral"),
            EventId::StoneOfAllTime => act == 1 && held_potions >= 1,
            EventId::Symbiote => act > 0,
            EventId::TheFutureOfPotions => held_potions >= 2,
            EventId::WaterloggedScriptorium => run.gold >= 55,
            EventId::WelcomeToWongos => act == 1 && run.gold >= 100,
            EventId::WhisperingHollow => run.gold >= 44,

            // events/shared.py
            EventId::Amalgamator => {
                let strikes = run
                    .deck
                    .iter()
                    .filter(|card| {
                        card.rarity == CardRarity::Basic
                            && card.id != CardId::AscendersBane
                            && card.card_type != CardType::Quest
                            && !card.has_kw(KW_ETERNAL)
                            && card.has_tag(CardTag::Strike)
                    })
                    .count();
                let defends = run
                    .deck
                    .iter()
                    .filter(|card| {
                        card.rarity == CardRarity::Basic
                            && card.id != CardId::AscendersBane
                            && card.card_type != CardType::Quest
                            && !card.has_kw(KW_ETERNAL)
                            && card.has_tag(CardTag::Defend)
                    })
                    .count();
                strikes >= 2 && defends >= 2
            }
            EventId::ByrdonisNest => !self.has_event_pet(),
            // C# GraveOfTheForgotten.IsAllowed / RoundTeaParty.IsAllowed.
            EventId::GraveOfTheForgotten => self.combat.as_ref().map_or_else(
                || can_enchant("SoulsPower"),
                |c| c.players.iter().all(|player| c.player_deck(player.creature)
                    .iter().any(|card| Self::event_can_enchant(card, "SoulsPower"))),
            ),
            EventId::RoundTeaParty => run.hp >= 12 && self.combat.as_ref().is_none_or(
                |c| c.players.iter().skip(1).all(|player| c.cr(player.creature).hp >= 12),
            ),
            EventId::ColossalFlower => run.hp >= 19,
            EventId::DenseVegetation => true,
            EventId::PunchOff => selection_floor >= 6,
            EventId::TrashHeap => run.hp > 5,
            EventId::UnrestSite => run.hp * 100 <= run.max_hp * 70,
            EventId::WoodCarvings => run.deck.iter().any(|card| {
                card.rarity == CardRarity::Basic
                    && card.id != CardId::AscendersBane
                    && card.card_type != CardType::Quest
                    && !card.has_kw(KW_ETERNAL)
            }),
            EventId::ZenWeaver => run.gold >= 125,

            // Never selected from a random event pool.
            EventId::Neow
            | EventId::TheArchitect
            | EventId::WarHistorianRepy
            | EventId::DeprecatedEvent
            | EventId::DeprecatedAncientEvent => false,
            _ => true,
        }
    }

    fn refresh_event_option_locks(&mut self) {
        // C# GraveOfTheForgotten/Symbiote/WoodCarvings.GenerateInitialOptions.
        let Some(event) = self.current_event.as_ref() else { return };
        if event.id == EventId::FakeMerchant {
            let gold = self.run_ctx().gold;
            let event = self.current_event.as_mut().unwrap();
            for (index, option) in event.options.iter_mut().take(6).enumerate() {
                option.enabled = event.flags & (1 << index) == 0 && gold >= event.ints[index];
            }
            return;
        }
        if event.id == EventId::SelfHelpBook {
            // C# SelfHelpBook.GenerateInitialOptions / DeckFilter: keep locked
            // options, and use NO_OPTIONS only if all three typed pools are empty.
            let enabled = [
                (CardType::Attack, "Sharp"),
                (CardType::Skill, "Nimble"),
                (CardType::Power, "Swift"),
            ].map(|(ty, enchantment)| self.run_ctx().deck.iter()
                .any(|c| c.card_type == ty && Self::event_can_enchant(c, enchantment)));
            let event = self.current_event.as_mut().unwrap();
            if enabled.iter().any(|&e| e) {
                for (option, enabled) in event.options.iter_mut().zip(enabled) {
                    option.enabled = enabled;
                }
            } else {
                event.options.clear();
                event.options.push(super::events::EventOption::new(
                    "skip_book", "No Options", "No valid cards"));
            }
            return;
        }
        let (option_id, enchantment) = match event.id {
            EventId::GraveOfTheForgotten => (EventOptionId::GraveOfTheForgottenInitialConfront, "SoulsPower"),
            EventId::Symbiote => (EventOptionId::SymbioteInitialApproach, "Corrupted"),
            EventId::WoodCarvings => (EventOptionId::WoodCarvingsInitialSnake, "Slither"),
            _ => return,
        };
        let enabled = self.run_ctx().deck.iter().any(|card| Self::event_can_enchant(card, enchantment));
        if let Some(option) = self.current_event.as_mut().unwrap().options.iter_mut()
            .find(|option| option.option_id.is(option_id)) {
            option.enabled = enabled;
        }
    }

    fn initialize_colorful_philosophers(&mut self) -> Result<(), &'static str> {
        let mut event = self.current_event.take().ok_or("Colorful Philosophers event missing")?;
        const POOLS: [crate::character::Character; 5] = [crate::character::Character::Necrobinder, crate::character::Character::Ironclad, crate::character::Character::Regent, crate::character::Character::Silent, crate::character::Character::Defect];
        let mut available: Vec<crate::character::Character> =
            POOLS.into_iter().filter(|&pool| pool != self.character).collect();
        while available.len() > 3 {
            let index = event.rng.choice_index(available.len());
            available.remove(index);
        }
        event.options.clear();
        event.ints[0] = available.len() as i32;
        for (index, pool) in available.into_iter().enumerate() {
            event.ints[index + 1] = POOLS.iter().position(|&candidate| candidate == pool)
                .ok_or("Colorful Philosophers pool missing")? as i32;
            let (id, label, description) = match index {
                0 => ("pool_1", philosopher_label(pool), philosopher_description(pool)),
                1 => ("pool_2", philosopher_label(pool), philosopher_description(pool)),
                2 => ("pool_3", philosopher_label(pool), philosopher_description(pool)),
                _ => unreachable!(),
            };
            event.options.push(EventOption::new(id, label, description));
        }
        self.current_event = Some(event);
        Ok(())
    }

    fn initialize_stone_of_all_time(&mut self) -> Result<(), &'static str> {
        // C# Player.Potions excludes empty slots, preserving inventory order.
        let slots: Vec<_> = self.run_ctx().potions.iter().enumerate()
            .filter_map(|(i, p)| p.is_some().then_some(i)).collect();
        let can_push = self.run_ctx().deck.iter()
            .any(|c| Self::event_can_enchant(c, "Vigorous"));
        let event = self.current_event.as_mut().ok_or("Stone event missing")?;
        event.ints[0] = if slots.is_empty() { -1 } else {
            slots[event.rng.choice_index(slots.len())] as i32
        };
        event.options.replace([
            EventOption::new("lift", "Lift", "Discard a potion, gain 10 Max HP"),
            EventOption::new("push", "Push", "Take 6 damage, enchant a card with Vigorous +8"),
        ]);
        event.options[0].enabled = event.ints[0] >= 0;
        event.options[1].enabled = can_push;
        Ok(())
    }

    fn initialize_relic_trader(&mut self) -> Result<(), &'static str> {
        let mut owned: Vec<usize> = self.run_ctx().relics.iter().enumerate()
            .filter_map(|(i, relic)| Self::event_relic_is_tradable(relic).then_some(i)).collect();
        // AbstractModel.CompareTo -> ModelId: canonical relic ID order,
        // then ListExtensions.StableShuffle's descending Fisher-Yates.
        owned.sort_by_key(|&i| self.run_ctx().relics[i].id.name());
        let mut event = self.current_event.take().ok_or("Relic Trader event missing")?;
        event.rng.shuffle(&mut owned);
        event.options.clear();
        event.relics.clear();
        if owned.is_empty() {
            event.options.push(EventOption::new("proceed", "Proceed", ""));
        } else {
            // CalculateVars evaluates OwnedRelics before NewRelics. Pull all
            // three, even if fewer than three owned relics can be offered.
            for _ in 0..3 { event.relics.push(self.pull_player_relic(None, false)); }
            for (index, (&owned_index, option)) in owned.iter().take(3)
                .zip(["trade_0", "trade_1", "trade_2"]).enumerate() {
                event.ints[index] = owned_index as i32;
                event.options.push(EventOption::new(option, ["Trade Relic 1","Trade Relic 2","Trade Relic 3"][index],
                    "Swap an owned relic for a new one"));
            }
        }
        self.current_event = Some(event);
        Ok(())
    }

    fn initialize_ranwid(&mut self) -> Result<(), &'static str> {
        let potions: Vec<_> = self.run_ctx().potions.iter().enumerate()
            .filter_map(|(i, p)| p.is_some().then_some(i)).collect();
        let relics: Vec<_> = self.run_ctx().relics.iter().enumerate()
            .filter_map(|(i, r)| Self::event_relic_is_tradable(r).then_some(i)).collect();
        let event = self.current_event.as_mut().ok_or("Ranwid event missing")?;
        // C# GenerateInitialOptions: NextItem(Potions), then NextItem(tradable
        // Relics), both in inventory order, NOT StableShuffle or ID order.
        event.ints[0] = if potions.is_empty() { -1 } else {
            potions[event.rng.choice_index(potions.len())] as i32
        };
        event.ints[1] = if relics.is_empty() { -1 } else {
            relics[event.rng.choice_index(relics.len())] as i32
        };
        event.options.replace([
            EventOption::new("potion", "Give a Potion", "Lose a potion, gain a relic"),
            EventOption::new("gold", "Give 100 Gold", "Lose 100g, gain a relic"),
            EventOption::new("relic", "Give a Relic", "Lose a relic, gain 2 relics"),
        ]);
        event.options[0].enabled = event.ints[0] >= 0;
        event.options[2].enabled = event.ints[1] >= 0;
        Ok(())
    }

    fn initialize_nonupeipe(&mut self) -> Result<(), &'static str> {
        self.heal_before_ancient_event()?;
        let allowed=self.observation_nonupeipe_bracelet_allowed();
        Self::populate_nonupeipe_options(self.current_event.as_mut().ok_or("Nonupeipe event missing")?,allowed);
        Ok(())
    }

    pub(in crate::run) fn populate_nonupeipe_options(event:&mut EventState,bracelet_allowed:bool){
        // Nonupeipe.OptionPool insertion order, followed by the conditional
        // tenth option. Take(3) occurs only AFTER shuffling the entire list.
        let mut pool=vec![
            (RelicId::BlessedAntler,"Blessed Antler"),(RelicId::BrilliantScarf,"Brilliant Scarf"),
            (RelicId::DelicateFrond,"Delicate Frond"),(RelicId::DiamondDiadem,"Diamond Diadem"),
            (RelicId::FurCoat,"Fur Coat"),(RelicId::Glitter,"Glitter"),
            (RelicId::JewelryBox,"Jewelry Box"),(RelicId::LoomingFruit,"Looming Fruit"),
            (RelicId::SignetRing,"Signet Ring")];
        if bracelet_allowed {
            pool.push((RelicId::BeautifulBracelet,"Beautiful Bracelet"));
        }
        event.rng.shuffle(&mut pool);
        for ((id,label),option) in pool.into_iter().take(3).zip(["relic_1","relic_2","relic_3"]) {
            event.relics.push(id);
            event.options.push(EventOption::new(option,label,"Gain an ancient relic"));
        }
    }

    fn initialize_fake_merchant(&mut self) -> Result<(), &'static str> {
        let mut event = self.current_event.take().ok_or("Fake Merchant event missing")?;
        let mut relics = [
            (RelicId::FakeAnchor, "Fake Anchor"), (RelicId::FakeBloodVial, "Fake Blood Vial"),
            (RelicId::FakeHappyFlower, "Fake Happy Flower"), (RelicId::FakeLeesWaffle, "Fake Lee's Waffle"),
            (RelicId::FakeMango, "Fake Mango"), (RelicId::FakeOrichalcum, "Fake Orichalcum"),
            (RelicId::FakeSneckoEye, "Fake Snecko Eye"), (RelicId::FakeStrikeDummy, "Fake Strike Dummy"),
            (RelicId::FakeVenerableTeaSet, "Fake Venerable Tea Set"),
        ];
        event.rng.shuffle(&mut relics);
        event.options.clear();
        event.relics.clear();
        for (index, ((id, label), option)) in relics.into_iter().take(6)
            .zip(["buy_0", "buy_1", "buy_2", "buy_3", "buy_4", "buy_5"]).enumerate() {
            // MerchantRelicEntry.CalcCost: float32 multiplication, then Math.Round
            // ties-to-even. MerchantEntry.Cost/Wrapper apply NO price modifiers
            // and NO Courier restock in an EventRoom (only MerchantRoom).
            let factor = self.run_ctx_mut().rngs.shops.next_float_range(0.85, 1.15) as f32;
            let cost = (50.0_f32 * factor).round_ties_even() as i32;
            event.ints[index] = cost;
            event.relics.push(id);
            event.options.push(EventOption::new(option, label, "")
                .with_dynamic_description(super::events::EventDescription::FakeMerchantCost(cost)));
        }
        event.options.push(EventOption::new("leave", "Leave", ""));
        self.current_event = Some(event);
        self.refresh_event_option_locks();
        Ok(())
    }

    /// C# Owner.ExtraFields.WongoPoints: this visit's earnings, not saved total.
    pub fn wongo_points_earned(&self) -> i32 { self.wongo_points_earned }

    fn pull_wongo_relic(&mut self, rarity: usize) -> RelicId {
        self.prune_player_relic_bag();
        // GetAvailableDeque escalates on no matching shop-allowed entry;
        // disallowed-in-shop entries remain in the bag for non-shop pulls.
        for bucket in rarity..3 {
            if let Some(i) = self.relic_bags.player[0][bucket].iter()
                .position(|id| RELIC_INFO[id.index()].shops) {
                let id = self.relic_bags.player[0][bucket].remove(i);
                for bag in &mut self.relic_bags.shared { bag.retain(|other| *other != id); }
                return id;
            }
        }
        RelicId::Circlet
    }

    fn initialize_wongo(&mut self) -> Result<(), &'static str> {
        if !self.options.saved_wongo_points.is_some_and(|p| p >= 0) { return Ok(()); }
        let featured = self.pull_wongo_relic(2);
        let gold = self.run_ctx().gold;
        let event = self.current_event.as_mut().ok_or("Wongo event missing")?;
        event.relics.push(featured);
        event.options.replace([
            EventOption::new("bargain_bin", "Bargain Bin", "Common relic"),
            EventOption::new("featured", "Featured Item", "Rare relic"),
            EventOption::new("mystery", "Mystery Box", "Mystery ticket relic"),
            EventOption::new("leave", "Leave", "Downgrade a random upgraded card"),
        ]);
        for (option, cost) in event.options.iter_mut().zip([100, 200, 300]) {
            option.enabled = gold >= cost;
        }
        Ok(())
    }

    fn finish_wongo_purchase(&mut self) -> Result<(), &'static str> {
        if self.unported.is_some() || self.run_ctx().hp <= 0 { return Ok(()); }
        let points = self.current_event.as_ref().ok_or("Wongo continuation missing")?.ints[0];
        let saved = self.options.saved_wongo_points.ok_or("Wongo saved points missing")?;
        self.wongo_points_earned = points;
        if saved % 2000 + points >= 2000 {
            self.obtain_event_relic(RelicId::WongoCustomerAppreciationBadge)?;
        }
        self.current_event = None;
        self.phase = RunPhase::MapChoice;
        Ok(())
    }

    fn downgrade_wongo_card(&mut self, index: usize) -> Result<(), &'static str> {
        Self::downgrade_event_card(self.run_ctx_mut(), index)
    }

    pub(in crate::run) fn downgrade_event_card(run: &mut RunCtx, index: usize) -> Result<(), &'static str> {
        run.deck[index] = Self::downgrade_wongo_template(&run.deck[index])?;
        Ok(())
    }

    pub(crate) fn downgrade_wongo_template(old: &Card) -> Result<Card, &'static str> {
        // CardModel.DowngradeInternal resets dynamic vars/canonical cost/local
        // keywords, then runs AfterDowngraded -> Enchantment.ModifyCard ->
        // Affliction.AfterApplied. Neither ToMutable allocates a run card ID.
        let reference = Card::from_factory(old.id, true, 0).ok_or("Wongo upgraded reference missing")?;
        let base = Card::from_factory(old.id, false, 0).ok_or("Wongo downgrade factory missing")?;
        let damage_growth = if SELF_MUTATING_DAMAGE.contains(&old.id) {
            old.damage.unwrap_or(0) - reference.damage.unwrap_or(0)
        } else { 0 };
        let block_growth = if SELF_MUTATING_BLOCK.contains(&old.id) {
            old.var(VarKey::Block).or(old.block).unwrap_or(0)
                - reference.var(VarKey::Block).or(reference.block).unwrap_or(0)
        } else { 0 };
        let mut card = old.clone(); // identity, saved progress, modifiers, affliction and enchantment
        card.upgraded = false;
        card.damage = base.damage;
        card.block = base.block;
        card.vars = base.vars;
        card.keywords = base.keywords;
        card.set_base_energy_cost(base.original_cost); // retains local cost modifiers
        card.star_cost = base.star_cost;
        // Nine C# AfterDowngraded overrides. The additive damage/block families
        // retain the same saved growth also preserved by upgrade_deck_card.
        if SELF_MUTATING_DAMAGE.contains(&card.id) {
            card.damage = Some(card.damage.unwrap_or(0) + damage_growth);
            if let Some((_, value)) = card.vars.iter_mut().find(|(k,_)| *k == VarKey::Damage) {
                *value = card.damage.unwrap();
            }
        }
        if SELF_MUTATING_BLOCK.contains(&card.id) {
            card.block = Some(card.block.unwrap_or(0) + block_growth);
            if let Some((_, value)) = card.vars.iter_mut().find(|(k,_)| *k == VarKey::Block) {
                *value = card.block.unwrap();
            }
        }
        // SovereignBlade.AfterDowngraded sets absolute CurrentDamage/Repeats;
        // its forged repeat count is independent of the upgrade level.
        if card.id == CardId::SovereignBlade {
            card.damage = old.damage;
            for key in [VarKey::Damage, VarKey::Repeat] {
                if let (Some(value), Some((_, out))) = (old.var(key),
                    card.vars.iter_mut().find(|(k,_)| *k == key)) { *out = value; }
            }
        }
        for &(name, amount) in &old.enchantments {
            if !crate::card::enchant::apply_static_enchantment(&mut card, name, amount) {
                return Err("Wongo enchantment capacity exceeded");
            }
        }
        // All concrete C# afflictions inherit AfflictionModel.AfterApplied's
        // no-op. Keep their instance/amount; do not replay combat application
        // effects such as adding Hex's Ethereal after canonical keyword reset.
        Ok(card)
    }

    fn resume_ranwid_pickups(&mut self) -> Result<(), &'static str> {
        // Each await Obtain must finish before the NEXT rarity roll / bag pull.
        if self.unported.is_some() || self.run_ctx().hp <= 0 { return Ok(()); }
        while self.current_event.as_ref().is_some_and(|e| e.ints[2] > 0) {
            if self.unported.is_some() || self.run_ctx().hp <= 0 { return Ok(()); }
            self.current_event.as_mut().unwrap().ints[2] -= 1;
            self.phase = RunPhase::MapChoice;
            self.reward = RewardState::default();
            let id = self.pull_player_relic(None, false);
            self.obtain_event_relic(id)?;
            if self.pending_run_choice.is_some() || self.phase == RunPhase::CardReward
                || self.phase == RunPhase::RunOver
                || self.unported.is_some() { return Ok(()); }
        }
        self.current_event = None;
        self.phase = RunPhase::MapChoice;
        Ok(())
    }

    fn enter_event_room(&mut self, ancient: bool) -> Result<(), &'static str> {
        let id = if ancient {
            let act = &mut self.acts[self.current_act];
            act.event_cursor += 1;
            act.rooms.ancient.ok_or("ancient event missing")?
        } else {
            let pool = self.acts[self.current_act].rooms.events.clone();
            if pool.is_empty() {
                return Err("event pool empty");
            }
            let start = self.acts[self.current_act].event_cursor;
            let mut index = start;
            let mut selected = None;
            for _ in 0..pool.len() {
                let candidate = pool[index % pool.len()];
                if candidate == EventId::LuminousChoir && !self.visited_event_ids.iter().any(|&seen| seen == candidate) {
                    // HasAvailableRelics calls GetAvailableDeque, which prunes
                    // disallowed relics. Player.CreateForNewRun uses new
                    // RelicGrabBag(): refresh=false; solo has no MP fallback.
                    self.prune_player_relic_bag();
                }
                if !self.visited_event_ids.iter().any(|&seen| seen == candidate) && self.event_is_allowed(candidate)
                {
                    selected = Some(candidate);
                    break;
                }
                index += 1;
            }
            let candidate = selected.unwrap_or(pool[index % pool.len()]);
            self.acts[self.current_act].event_cursor = index + 1;
            candidate
        };

        if !self.visited_event_ids.contains(&id) {
            self.visited_event_ids.push(id);
        }
        let run_seed = self.run_ctx().rngs.seed;
        let (gold, hp, max_hp, has_regal_pillow) = {
            let run = self.run_ctx();
            (
                run.gold,
                run.hp,
                run.max_hp,
                run.has_owned_relic(crate::combat::PLAYER, RelicId::RegalPillow),
            )
        };
        let rest_heal = max_hp * 3 / 10 + if has_regal_pillow { 15 } else { 0 };
        self.current_event =
            super::events::initialize(id, run_seed, gold, rest_heal, hp, max_hp).or_else(|| {
                Some(EventState::new(
                    id,
                    Rng::new(run_seed.wrapping_add(deterministic_hash64(id.name()))),
                ))
            });
        if id == EventId::WelcomeToWongos {
            self.initialize_wongo()?;
        } else if id == EventId::FakeMerchant {
            self.initialize_fake_merchant()?;
        } else if id == EventId::RanwidTheElder {
            self.initialize_ranwid()?;
        } else if id == EventId::RelicTrader {
            self.initialize_relic_trader()?;
        } else if id == EventId::Vakuu {
            self.heal_before_ancient_event()?;
        } else if id == EventId::Nonupeipe {
            self.initialize_nonupeipe()?;
        } else if id == EventId::StoneOfAllTime {
            self.initialize_stone_of_all_time()?;
        } else if id == EventId::Orobas {
            self.initialize_orobas()?;
        } else if id == EventId::Tezcatara {
            self.initialize_tezcatara()?;
        } else if id == EventId::Darv {
            self.initialize_darv()?;
        } else if id == EventId::Pael {
            self.initialize_pael()?;
        } else if id==EventId::Tanx {
            self.heal_before_ancient_event()?;
            let deck=self.run_ctx().deck.clone();
            super::tanx::options(self.current_event.as_mut().unwrap(),&deck);
        } else if id == EventId::TheFutureOfPotions {
            self.initialize_future_potions()?;
        } else if id == EventId::ColorfulPhilosophers {
            self.initialize_colorful_philosophers()?;
        }
        if id == EventId::EndlessConveyor {
            self.resume_conveyor();
        }
        if id == EventId::PunchOff {
            self.prepare_punch_off()?;
        }
        if id==EventId::TheLanternKey {
            self.prepare_event_combat(EncounterId::MysteriousKnightEventEncounter)?;
        }
        if id == EventId::SlipperyBridge {
            let mut event = self.current_event.take().unwrap();
            self.roll_bridge_card(&mut event)?;
            self.current_event = Some(event);
        }
        self.refresh_event_option_locks();
        self.phase = RunPhase::Event;
        if self
            .current_event
            .as_ref()
            .is_some_and(|event| !event.options.is_empty())
        {
            self.unported = None;
        } else {
            self.unported = Some(super::events::unported_reason(id));
        }
        Ok(())
    }

    fn remove_relic_from_bags(&mut self, id: RelicId) -> Result<(), &'static str> {
        self.relic_bags.remove_obtained(crate::combat::PLAYER as usize, id)
    }

    fn obtain_event_relic(&mut self, id: RelicId) -> Result<(), &'static str> {
        self.remove_relic_from_bags(id)?;
        if self.run_ctx().total_floor == 1 && neow_rewards::NEOW_RELICS.contains(&id) {
            let mut relic = Relic::new(id);
            relic.floor_added_to_deck = self.run_ctx().total_floor;
            self.run_ctx_mut().relics.push(relic);
            return self.apply_neow_pickup(id);
        }
        match id {
            RelicId::FakeLeesWaffle | RelicId::FakeMango => {
                shop::attach_relic(self.run_ctx_mut(),id);
                if id == RelicId::FakeLeesWaffle {
                    self.apply_event_effect(EventEffect::Heal(self.run_ctx().max_hp / 10))?;
                } else {
                    self.apply_event_effect(EventEffect::GainMaxHp(3))?;
                }
            }
            // C# BigMushroom.AfterObtained / FragrantMushroom.AfterObtained.
            RelicId::BigMushroom | RelicId::FragrantMushroom => {
                shop::attach_relic(self.run_ctx_mut(),id);
                if id == RelicId::BigMushroom {
                    self.apply_event_effect(EventEffect::GainMaxHp(20))?;
                } else {
                    self.apply_event_effect(EventEffect::LoseHp(15))?;
                    if self.run_ctx().hp > 0 {
                        self.apply_event_effect(EventEffect::UpgradeRandomDeckCards(2))?;
                    }
                }
            }
            _ => {
                let pickup = shop::add_relic(self.run_ctx_mut(), id);
                if self.offer_nr_pickup(id)? { return Ok(()); }
                if let Err(reason) = pickup {
                    self.unported = Some(reason);
                }
            }
        }
        Ok(())
    }

    fn has_event_death_prevention(&self) -> bool {
        self.run_ctx().owned_relics(crate::combat::PLAYER).any(|r|
            r.id == RelicId::LizardTail && r.enabled && !r.flag1)
            || self.run_ctx().potions.contains(&Some(PotionId::Fairyinabottle))
    }

    fn apply_event_effect(&mut self, effect: EventEffect) -> Result<(), &'static str> {
        match effect {
            EventEffect::LoseHp(amount) => {
                // C# CreatureCmd.Damage -> Kill -> Hook.ShouldDie also runs
                // outside combat. LizardTail.ShouldDieLate / FairyInABottle.
                // ShouldDie can prevent death; do not approximate those hooks
                // with raw HP loss and then enter the terminal-death guards.
                let multiplayer = self.is_multiplayer();
                if let Err(reason) = slippery_bridge::damage_event_player(self.run_ctx_mut(), amount, multiplayer) {
                    self.unported = Some(reason);
                    return Err(reason);
                }
                if self.run_ctx().hp == 0 {
                    self.phase = RunPhase::RunOver;
                }
            }
            EventEffect::Heal(amount) => {
                let run = self.run_ctx_mut();
                run.hp = (run.hp + amount.max(0)).min(run.max_hp);
            }
            EventEffect::GainMaxHp(amount) => {
                let run = self.run_ctx_mut();
                let amount = amount.max(0);
                run.max_hp += amount;
                run.hp += amount;
            }
            EventEffect::LoseMaxHp(amount) => {
                let run = self.run_ctx_mut();
                run.max_hp = (run.max_hp - amount.max(0)).max(1);
                run.hp = run.hp.min(run.max_hp);
            }
            EventEffect::GainGold(amount) => { treasure::gain_gold(self.run_ctx_mut(), amount); }
            EventEffect::LoseGold(amount) => {
                let run = self.run_ctx_mut();
                run.gold = (run.gold - amount.max(0)).max(0);
            }
            EventEffect::AddCard(id) => self.add_card_to_deck(id)?,
            EventEffect::AddRelic(id) => self.obtain_event_relic(id)?,
            EventEffect::RollTrashRelic => return Err("event RNG required"),
            EventEffect::UpgradeRandomDeckCards(count) => {
                let mut rng = self.run_ctx().rngs.niche.clone();
                self.upgrade_random_event_cards(&mut rng, count);
                self.run_ctx_mut().rngs.niche = rng;
            }
        }
        Ok(())
    }

    fn upgrade_random_event_cards(&mut self, rng: &mut Rng, count: usize) {
        let len = self.run_ctx().deck.len();
        let mut candidates = Vec::new();
        for index in 0..len {
            if self.run_ctx_mut().is_deck_card_upgradable(index) {
                candidates.push(index as crate::card::KRef);
            }
        }
        // C# ListExtensions.StableShuffle sorts with .NET's unstable IntroSort.
        crate::flow::dotnet_sort_cards(&mut candidates, &self.run_ctx().deck);
        rng.shuffle(&mut candidates);
        for index in candidates.into_iter().take(count) {
            self.run_ctx_mut().upgrade_deck_card(index as usize);
        }
    }

    fn take_event_choice<S: EventOptionSelector>(&mut self, option_id: S) -> Result<(), &'static str> {
        if self.phase != RunPhase::Event {
            return Err("not in event");
        }
        let option_id = {
            let event = self.current_event.as_ref().ok_or("event is unported")?;
            option_id.resolve(event)
        };
        if self.current_event.as_ref().is_some_and(|event| event.id == EventId::TheArchitect) {
            let event = self.current_event.as_mut().unwrap();
            let expected = if event.ints[0] > 1 {
                EventOptionId::TheArchitectDialogueCurrentLineIndex
            } else {
                EventOptionId::TheArchitectProceed
            };
            if !option_id.is(expected) { return Err("invalid Architect option"); }
            if expected == EventOptionId::TheArchitectProceed {
                self.current_event = None;
                self.phase = RunPhase::RunOver;
            } else {
                event.ints[0] -= 1;
                if let super::events::native_identity::EventParameter::Counter(line) = event.options.page.parameter {
                    event.options.page.parameter = super::events::native_identity::EventParameter::Counter(line + 1);
                }
                event.options.clear();
                event.options.push(EventOption::new(
                    if event.ints[0] > 1 { "advance" } else { "proceed" }, "Continue", ""));
            }
            return Ok(());
        }
        if self.current_event.as_ref().is_some_and(|event| event.id == EventId::TheFutureOfPotions) {
            if !self.current_event.as_ref().unwrap().options.iter()
                .any(|option| option.option_id == option_id && option.enabled)
            {
                return Err("invalid event option");
            }
            return self.trade_future_potion(option_id);
        }
        self.replay_event=self.current_event.clone();
        let mut event = self.current_event.take().ok_or("event is unported")?;
        if !event
            .options
            .iter()
            .any(|option| option.option_id == option_id && option.enabled)
        {
            self.current_event = Some(event);
            return Err("invalid event option");
        }
        if event.id == EventId::Orobas {
            return self.choose_orobas(event, option_id);
        }
        if event.id == EventId::TinkerTime {
            return self.choose_tinker_time(event, option_id);
        }
        if event.id == EventId::Trial {
            return self.choose_trial(event, option_id);
        }
        if event.id == EventId::Reflections {
            return self.choose_reflections(event, option_id);
        }
        if event.id == EventId::BattlewornDummy {
            return self.choose_battleworn_dummy(event, option_id);
        }
        if event.id == EventId::StoneOfAllTime {
            self.unported = None;
            if option_id.is(EventOptionId::StoneOfAllTimeInitialLift) {
                let slot = usize::try_from(event.ints[0]).map_err(|_| "Stone potion missing")?;
                self.run_ctx_mut().potions[slot] = None;
                self.apply_event_effect(EventEffect::GainMaxHp(10))?;
                event.rng.next_int_exclusive(0, 100);
                self.phase = RunPhase::MapChoice;
            } else {
                self.apply_event_effect(EventEffect::LoseHp(6))?;
                if self.phase == RunPhase::RunOver { return Ok(()); }
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i, c)| Self::event_can_enchant(c, "Vigorous").then_some(i)).collect();
                if candidates.len() <= 1 {
                    if let Some(&i) = candidates.first() {
                        if !crate::card::enchant::apply_static_enchantment(
                            &mut self.run_ctx_mut().deck[i], "Vigorous", 8)
                        { return Err("Stone enchantment capacity exceeded"); }
                    }
                    event.rng.next_int_exclusive(0, 100);
                    self.phase = RunPhase::MapChoice;
                } else {
                    self.current_event = Some(event);
                    self.begin_run_card_choice("event.stone_of_all_time.enchant",
                        "Choose a card to enchant with Vigorous", &candidates, 1, 1, false,
                        RunChoiceEffect::Enchant { name: EnchantmentId::Vigorous, amount: 8 },
                        ChoiceResume::MapChoice);
                }
            }
            return Ok(());
        }
        if event.id == EventId::Tezcatara {
            return self.choose_tezcatara(event, option_id);
        }
        if event.id == EventId::RelicTrader {
            self.phase = RunPhase::MapChoice;
            self.unported = None;
            if option_id.is(EventOptionId::RelicTraderProceed) { return Ok(()); }
            let index = event.options.iter().position(|o| o.option_id == option_id)
                .ok_or("Relic Trader option missing")?;
            let owned_index = event.ints[index] as usize;
            // Player.RemoveRelicInternal removes the actual instance. C#
            // RelicModel.AfterRemoved is a no-op; no relic overrides it.
            self.run_ctx_mut().relics.remove(owned_index);
            self.obtain_event_relic(event.relics[index])?;
            return Ok(());
        }
        if event.id == EventId::FakeMerchant {
            if option_id.is(EventOptionId::FakeMerchantNFakeMerchantHideScreen) {
                self.phase = RunPhase::MapChoice;
                self.unported = None;
                return Ok(());
            }
            let index = event.options.iter().position(|o| o.option_id == option_id)
                .filter(|&i| i < event.relics.len()).ok_or("Fake Merchant purchase missing")?;
            let price = event.ints[index];
            self.apply_event_effect(EventEffect::LoseGold(price))?;
            self.obtain_event_relic(event.relics[index])?;
            // MerchantEntry.Wrapper clears this slot after pickup and then
            // invokes AfterItemPurchased (MawBank), even outside MerchantRoom.
            event.flags |= 1 << index;
            shop::post_purchase(self.run_ctx_mut(), price);
            self.current_event = Some(event);
            self.refresh_event_option_locks();
            return Ok(());
        }
        if event.id == EventId::WelcomeToWongos {
            if option_id.is(EventOptionId::WelcomeToWongosInitialLeave) {
                let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)| c.upgraded.then_some(i)).collect();
                if !candidates.is_empty() {
                    let i = candidates[event.rng.choice_index(candidates.len())];
                    self.downgrade_wongo_card(i)?;
                }
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            let (cost, points) = match option_id.id {
                EventOptionId::WelcomeToWongosInitialBargainBin => (100, 32),
                EventOptionId::WelcomeToWongosInitialFeaturedItem => (200, 16),
                EventOptionId::WelcomeToWongosInitialMysteryBox => (300, 8),
                _ => return Err("invalid Wongo option"),
            };
            self.apply_event_effect(EventEffect::LoseGold(cost))?;
            let id = match option_id.id {
                EventOptionId::WelcomeToWongosInitialBargainBin => self.pull_wongo_relic(0),
                EventOptionId::WelcomeToWongosInitialFeaturedItem => event.relics[0],
                _ => RelicId::WongosMysteryTicket,
            };
            event.ints[0] = points;
            self.current_event = Some(event);
            self.phase = RunPhase::MapChoice;
            self.obtain_event_relic(id)?;
            if self.pending_run_choice.is_none() && self.phase != RunPhase::CardReward {
                self.finish_wongo_purchase()?;
            }
            return Ok(());
        }
        if event.id == EventId::RanwidTheElder {
            match option_id.id {
                EventOptionId::RanwidTheElderInitialPotion => {
                    self.run_ctx_mut().potions[event.ints[0] as usize] = None;
                    event.ints[2] = 1;
                }
                EventOptionId::RanwidTheElderInitialGold => {
                    self.apply_event_effect(EventEffect::LoseGold(100))?;
                    event.ints[2] = 1;
                }
                EventOptionId::RanwidTheElderInitialRelic => {
                    // Same exact-instance removal path as Relic Trader;
                    // RelicModel.AfterRemoved has no relic overrides.
                    self.run_ctx_mut().relics.remove(event.ints[1] as usize);
                    event.ints[2] = 2;
                }
                _ => return Err("invalid Ranwid option"),
            }
            self.current_event = Some(event);
            self.unported = None;
            return self.resume_ranwid_pickups();
        }
        if event.id==EventId::Tanx {
            let index=event.options.iter().position(|o|o.option_id==option_id).ok_or("Tanx option missing")?;
            let id=event.relics[index];
            self.remove_relic_from_bags(id)?;
            shop::attach_relic(self.run_ctx_mut(),id);
            self.phase=RunPhase::MapChoice;self.unported=None;
            match id {
                RelicId::TanxsWhistle=>self.add_card_to_deck(CardId::Whistle)?,
                RelicId::Claws=>{
                    let candidates=self.transformable_deck_indices();
                    let max=candidates.len().min(6);
                    if max>0 {self.begin_run_card_choice("relic:CLAWS","Choose up to 6 cards to transform",
                        &candidates,0,max,true,RunChoiceEffect::TransformMaul,ChoiceResume::MapChoice);}
                }
                RelicId::TriBoomerang=>{
                    let candidates:Vec<_>=self.run_ctx().deck.iter().enumerate().filter_map(|(i,c)|
                        Self::event_can_enchant(c,"Instinct").then_some(i)).collect();
                    let count=candidates.len().min(3);
                    if count>0 {self.begin_run_card_choice("relic:TRI_BOOMERANG","Choose cards for Instinct",
                        &candidates,count,count,false,RunChoiceEffect::Enchant{name:crate::identity::EnchantmentId::Instinct,amount:1},ChoiceResume::MapChoice);}
                }
                _=>{}
            }
            return Ok(());
        }
        if event.id == EventId::Nonupeipe {
            let index=event.options.iter().position(|o|o.option_id==option_id)
                .ok_or("Nonupeipe option missing")?;
            let id=*event.relics.get(index).ok_or("Nonupeipe relic missing")?;
            self.remove_relic_from_bags(id)?;
            let mut relic=Relic::new(id);
            relic.floor_added_to_deck=self.run_ctx().total_floor;
            if id==RelicId::FurCoat{
                let coords=self.acts[self.current_act].map.as_ref().unwrap()
                    .fur_coat_coords(self.run_ctx().rngs.seed,0,&self.run_ctx().relics);
                relic.counter=self.current_act as i32;
                relic.set_fur_coat_coords(&coords).map_err(|_|"Fur Coat coordinate capacity")?;
            }
            self.run_ctx_mut().relics.push(relic);
            self.phase=RunPhase::MapChoice;
            self.unported=None;
            match id {
                RelicId::JewelryBox=>self.add_card_to_deck(CardId::Apotheosis)?,
                RelicId::LoomingFruit=>self.apply_event_effect(EventEffect::GainMaxHp(31))?,
                RelicId::SignetRing=>self.apply_event_effect(EventEffect::GainGold(888))?,
                RelicId::BeautifulBracelet=>{
                    // IEnumerableExtensions.TakeRandom: complete Niche shuffle,
                    // then Take(4); no player selector or extra random choices.
                    let mut cards:Vec<_>=self.run_ctx().deck.iter().enumerate()
                        .filter_map(|(i,c)|Self::event_can_enchant(c,"Swift").then_some(i)).collect();
                    self.run_ctx_mut().rngs.niche.shuffle(&mut cards);
                    for index in cards.into_iter().take(4) {
                        if !crate::card::enchant::apply_static_enchantment(
                            &mut self.run_ctx_mut().deck[index],"Swift",2) {
                            return Err("Nonupeipe BeautifulBracelet Swift capacity");
                        }
                    }
                }
                _=>{}
            }
            return Ok(());
        }
        if event.id == EventId::Vakuu {
            let index = event.options.iter().position(|o| o.option_id == option_id)
                .ok_or("Vakuu option missing")?;
            let id = *event.relics.get(index).ok_or("Vakuu relic missing")?;
            self.remove_relic_from_bags(id)?;
            shop::attach_relic(self.run_ctx_mut(),id);
            self.phase = RunPhase::MapChoice;
            self.unported = None;
            match id {
                RelicId::BloodSoakedRose => self.add_card_to_deck(CardId::Enthralled)?,
                RelicId::SereTalon => {
                    // CreatureCmd.LoseMaxHp damages before SetMaxHp; lethal
                    // loss must go through the same death-prevention hooks.
                    let new_max = self.run_ctx().max_hp - 9;
                    let damage = self.run_ctx().hp - new_max;
                    if damage > 0 { self.apply_event_effect(EventEffect::LoseHp(damage))?; }
                    let run = self.run_ctx_mut();
                    run.max_hp = new_max.max(1);
                    run.hp = run.hp.min(run.max_hp); // Creature.SetMaxHpInternal, including revival.
                    for _ in 0..3 { self.add_card_to_deck(CardId::Wish)?; }
                }
                RelicId::DistinguishedCape => {
                    // CurseCardPool does not override FilterThroughEpochs.
                    // GetUnlockedCards -> CanBeGeneratedByModifiers -> Id order.
                    let mut curses: Vec<_> = super::card_pools::cards(CardPool::Curse, self.is_multiplayer())?
                        .into_iter().filter(|&id| card_info(id, false).is_some_and(|c| c.gen_modifiers))
                        .collect();
                    curses.sort_by_key(|id| id.name());
                    for _ in 0..2 {
                        let index = self.run_ctx_mut().rngs.niche.choice_index(curses.len());
                        self.add_card_to_deck(curses.remove(index))?;
                    }
                    for _ in 0..3 { self.add_card_to_deck(CardId::Apparition)?; }
                }
                RelicId::PreservedFog => {
                    // CardSelectCmd.FromDeckGeneric auto-selects if all eligible
                    // cards are required; otherwise suspend until removal, then Folly.
                    let candidates = self.removable_deck_indices();
                    let count = candidates.len().min(3);
                    if count == 0 {
                        self.add_card_to_deck(CardId::Folly)?;
                    } else {
                        self.begin_run_card_choice("relic.preserved_fog.pickup", "Choose 3 cards to remove",
                            &candidates, count, count, false,
                            RunChoiceEffect::RemoveAndAddCard(CardId::Folly), ChoiceResume::MapChoice);
                        if candidates.len() <= 3 {
                            let choice = self.pending_run_choice.take().unwrap();
                            self.apply_run_choice(choice, &(0..count).collect::<Vec<_>>())?;
                        }
                    }
                }
                RelicId::WhisperingEarring | RelicId::Fiddle | RelicId::ChoicesParadox |
                RelicId::MusicBox | RelicId::LordsParasol | RelicId::JeweledMask => {}
                _ => return Err("Vakuu invalid relic"),
            }
            return Ok(());
        }
        if event.id == EventId::Darv {
            return self.choose_darv(event, option_id);
        }
        if event.id==EventId::RoundTeaParty {
            // RoundTeaParty.PickFight only changes page; effects are awaited
            // by ContinueFight. EnjoyTea obtains before healing.
            if option_id.is(EventOptionId::RoundTeaPartyInitialPickFight) && event.ints[0]==0 {
                event.ints[0]=1;event.options.clear();
                event.options.set_page(crate::identity::EventPageId::RoundTeaPartyPickFight,
                    super::events::native_identity::EventParameter::None);
                event.options.push(EventOption::new("continue_fight","Continue Fight","Take 11 damage, gain a relic"));
                self.current_event=Some(event);
                return Ok(());
            }
            self.current_event=None;self.phase=RunPhase::MapChoice;
            if option_id.is(EventOptionId::RoundTeaPartyInitialEnjoyTea) && event.ints[0]==0 {
                self.obtain_event_relic(RelicId::RoyalPoison)?;
                self.apply_event_effect(EventEffect::Heal(self.run_ctx().max_hp))?;
            } else if option_id.is(EventOptionId::RoundTeaPartyPickFightContinueFight) && event.ints[0]==1 {
                self.apply_event_effect(EventEffect::LoseHp(11))?;
                let id=self.pull_player_relic(None,false);
                self.obtain_event_relic(id)?;
            } else {return Err("invalid Round Tea Party input");}
            return Ok(());
        }
        if event.id == EventId::EndlessConveyor {
            return self.choose_conveyor(event, option_id);
        }
        if event.id == EventId::SlipperyBridge {
            return self.choose_bridge(event, option_id);
        }
        if event.id == EventId::PunchOff {
            return self.choose_punch_off(event, option_id);
        }
        if event.id == EventId::CrystalSphere {
            self.unported = None;
            if event.crystal.is_none() {
                let picks = if option_id.is(EventOptionId::CrystalSphereInitialUncoverFuture) {
                    let cost = event.ints[0];
                    if self.run_ctx().gold < cost {
                        self.current_event = Some(event);
                        return Err("crystal sphere insufficient gold");
                    }
                    self.apply_event_effect(EventEffect::LoseGold(cost))?;
                    3
                } else {
                    self.add_card_to_deck(CardId::Debt)?;
                    6
                };
                event.crystal = Some(SphereState::new(&mut event.rng, picks));
            } else {
                let curses = event.crystal.as_mut().unwrap().click(option_id)?;
                for _ in 0..curses {
                    self.add_card_to_deck(CardId::Doubt)?;
                }
            }
            if event.crystal.as_ref().unwrap().picks_remaining > 0 {
                event.options.replace_sphere(event.crystal.as_ref().unwrap().options());
                self.current_event = Some(event);
                self.phase = RunPhase::Event;
                return Ok(());
            }

            let revealed = event.crystal.as_ref().unwrap().revealed_rewards();
            // C# OneOffSynchronizer.OfferCrystalSphereRewards materializes all
            // ToReward calls before RewardsSet populates any reward. Potion
            // construction draws immediately; relic/card/gold Populate draws
            // must therefore follow every potion draw, even for mixed loot.
            let mut potions = Vec::new();
            for kind in &revealed {
                if let SphereItemKind::Potion(rarity) = kind {
                    let pool: Vec<PotionId> = super::potion_pools::cards(self.character)
                        .ok_or("crystal sphere potion pool missing")?
                        .into_iter()
                        .filter(|id| POTION_INFO[id.index()].rarity == *rarity)
                        .collect();
                    if pool.is_empty() {
                        self.current_event = Some(event);
                        return Err("crystal sphere potion pool exhausted");
                    }
                    potions.push(pool[event.rng.choice_index(pool.len())]);
                }
            }
            let mut potions = potions.into_iter();
            self.reward = RewardState::default();
            self.reward.stay_in_container = true;
            for kind in revealed {
                match kind {
                    SphereItemKind::Gold(amount) => {
                        // SphereGoldReward.Populate consumes event RNG even for
                        // a fixed range, then GoldReward auto-selects before UI.
                        let amount = event.rng.next_int(amount as i64, amount as i64) as i32;
                        treasure::gain_gold(self.run_ctx_mut(), amount);
                    }
                    SphereItemKind::Relic => {
                        let roll = event.rng.next_float(1.0);
                        let rarity = if roll < 0.5 { 0 } else if roll < 0.83 { 1 } else { 2 };
                        let relic = self.pull_player_relic(Some(rarity), false);
                        self.reward.items.push(RewardItem {
                            kind: RewardItemKind::Relic(relic),
                            consumed: false,
                            skippable: true,
                            skip_completes: true,
                            add_injury_after: false,
                            neows_bones_final: false,
                        });
                    }
                    SphereItemKind::Potion(_) => {
                        let potion = potions.next().ok_or("crystal sphere potion missing")?;
                        self.reward.items.push(RewardItem {
                            kind: RewardItemKind::Potion(potion),
                            consumed: false,
                            skippable: true,
                            skip_completes: true,
                            add_injury_after: false,
                            neows_bones_final: false,
                        });
                    }
                    SphereItemKind::Card(rarity) => {
                        let item = self.make_crystal_sphere_card_reward(&mut event.rng, rarity)?;
                        self.reward.items.push(item);
                    }
                    SphereItemKind::Curse => unreachable!(),
                }
            }
            self.reward.sphere_rng = Some(event.rng);
            self.current_event = None;
            if self.reward.items.is_empty() {
                self.phase = RunPhase::MapChoice;
            } else {
                self.reward.active = None;
                self.phase = RunPhase::CardReward;
                self.advance_rewards();
            }
            return Ok(());
        }
        if event.id == EventId::DollRoom {
            self.unported = None;
            if let Some(index) = option_id.offer_index(EventOptionId::DollRoomTakeTake) {
                let relic = *event.relics.get(index).ok_or("invalid doll choice")?;
                self.current_event = None;
                self.obtain_event_relic(relic)?;
                if self.unported.is_none() && self.phase == RunPhase::Event {
                    self.phase = RunPhase::MapChoice;
                }
                return Ok(());
            }
            if option_id.is(EventOptionId::DollRoomInitialRandom) {
                let relic = super::events::doll_room::random(&mut event);
                self.current_event = None;
                self.obtain_event_relic(relic)?;
                if self.unported.is_none() && self.phase == RunPhase::Event {
                    self.phase = RunPhase::MapChoice;
                }
                return Ok(());
            }
            let take_time = option_id.is(EventOptionId::DollRoomInitialTakeSomeTime);
            let damage = if take_time { 5 } else { 15 };
            self.apply_event_effect(EventEffect::LoseHp(damage))?;
            if self.run_ctx().hp == 0 || self.unported.is_some() {
                self.current_event = None;
                return Ok(());
            }
            let count = if take_time { 2 } else { 3 };
            super::events::doll_room::offer(&mut event, count);
            event.options.set_page(if take_time { crate::identity::EventPageId::DollRoomTakeSomeTime }
                else { crate::identity::EventPageId::DollRoomExamine }, super::events::native_identity::EventParameter::None);
            self.current_event = Some(event);
            self.phase = RunPhase::Event;
            return Ok(());
        }
        if event.id == EventId::FieldOfManSizedHoles {
            self.unported = None;
            self.current_event = None;
            if option_id.is(EventOptionId::FieldOfManSizedHolesInitialResist) {
                let curse = CardId::Normality;
                let candidates = self.removable_deck_indices();
                let count = candidates.len().min(2);
                if candidates.len() <= 2 {
                    let selected: Vec<u32> = candidates.iter()
                        .map(|&index| self.run_ctx().deck[index].instance_id).collect();
                    self.run_ctx_mut().retain_run_deck_cards(|card| !selected.contains(&card.instance_id));
                    self.add_card_to_deck(curse)?;
                    self.phase = RunPhase::MapChoice;
                } else {
                    self.begin_run_card_choice(
                        "event.field_of_man_sized_holes.remove",
                        "Choose 2 cards to remove",
                        &candidates,
                        count,
                        count,
                        false,
                        RunChoiceEffect::RemoveAndAddCard(curse),
                        ChoiceResume::MapChoice,
                    );
                }
            } else {
                let candidates: Vec<usize> = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(index, card)| Self::event_can_enchant(card, "PerfectFit").then_some(index))
                    .collect();
                if candidates.len() <= 1 {
                    if let Some(&index) = candidates.first()
                        && !crate::card::enchant::apply_static_enchantment(
                            &mut self.run_ctx_mut().deck[index], "PerfectFit", 1)
                    {
                        return Err("Perfect Fit enchantment capacity exceeded");
                    }
                    self.phase = RunPhase::MapChoice;
                } else {
                    self.begin_run_card_choice(
                        "event.field_of_man_sized_holes.enchant",
                        "Choose a card to enchant with Perfect Fit",
                        &candidates,
                        1,
                        1,
                        false,
                        RunChoiceEffect::Enchant { name: EnchantmentId::PerfectFit, amount: 1 },
                        ChoiceResume::MapChoice,
                    );
                }
            }
            return Ok(());
        }
        if event.id == EventId::InfestedAutomaton {
            self.current_event = None;
            self.unported = None;
            let card = if option_id.is(EventOptionId::InfestedAutomatonInitialStudy) {
                self.generate_filtered_noncombat_card(Some(CardType::Power), None, None)?
            } else {
                self.generate_filtered_noncombat_card(None, Some(0), Some(false))?
            };
            self.add_card_instance_to_deck(card)?;
            if self.unported.is_none() {
                self.phase = RunPhase::MapChoice;
            }
            return Ok(());
        }
        if event.id == EventId::SpiritGrafter {
            self.current_event = None;
            self.unported = None;
            if option_id.is(EventOptionId::SpiritGrafterInitialLetItIn) {
                for effect in super::events::spirit_grafter::accept() {
                    self.apply_event_effect(effect)?;
                }
                if self.unported.is_none() {
                    self.phase = RunPhase::MapChoice;
                }
                return Ok(());
            }
            let candidates = self.upgradable_deck_indices();
            if candidates.len() <= 1 {
                if let Some(&index) = candidates.first() {
                    self.run_ctx_mut().upgrade_deck_card(index);
                }
                self.apply_event_effect(EventEffect::LoseHp(super::events::spirit_grafter::REJECTION_DAMAGE))?;
                if self.run_ctx().hp > 0 && self.unported.is_none() {
                    self.phase = RunPhase::MapChoice;
                }
            } else {
                self.begin_run_card_choice(
                    "event.spirit_grafter.upgrade",
                    "Choose a card to upgrade",
                    &candidates,
                    1,
                    1,
                    false,
                    RunChoiceEffect::UpgradeAndDamage(super::events::spirit_grafter::REJECTION_DAMAGE),
                    ChoiceResume::MapChoice,
                );
            }
            return Ok(());
        }
        if event.id == EventId::ColorfulPhilosophers {
            let index = option_id.offer_index(EventOptionId::ColorfulPhilosophersInitialCardPoolEnergyColorNameToUpperInvariant)
                .ok_or("invalid philosopher pool")?;
            const POOLS: [crate::character::Character; 5] = [crate::character::Character::Necrobinder, crate::character::Character::Ironclad, crate::character::Character::Regent, crate::character::Character::Silent, crate::character::Character::Defect];
            let pool_index = *event.ints.get(index + 1).ok_or("invalid philosopher pool")? as usize;
            let pool = *POOLS.get(pool_index).ok_or("invalid philosopher pool")?;
            self.current_event = None;
            self.unported = None;
            self.reward = RewardState::default();
            self.reward.stay_in_container = true;
            for rarity in [CardRarity::Common, CardRarity::Uncommon, CardRarity::Rare] {
                let item = self.make_noncombat_card_reward_with_pool_modifications(
                    3, pool.into(), Some(rarity), true, false, false, false)?;
                self.reward.items.push(item);
            }
            self.reward.active = None;
            self.phase = RunPhase::CardReward;
            return Ok(());
        }
        if event.id == EventId::TheLanternKey {
            self.unported = None;
            match super::events::lantern_key::choose(&mut event, option_id)? {
                super::events::lantern_key::Action::Return(effect) => {
                    self.current_event = None;
                    self.apply_event_effect(effect)?;
                    self.phase = RunPhase::MapChoice;
                }
                super::events::lantern_key::Action::Keep => {
                    event.options.set_page(crate::identity::EventPageId::TheLanternKeyKeepTheKey,
                        super::events::native_identity::EventParameter::None);
                    self.current_event = Some(event);
                    self.phase = RunPhase::Event;
                }
                super::events::lantern_key::Action::Fight => {
                    let iid = self.run_ctx_mut().new_card_id();
                    self.lantern_key_reward = Some(
                        Card::from_factory(super::events::lantern_key::CARD, false, iid)
                            .ok_or("Lantern Key card factory missing")?);
                    self.current_event = None;
                    self.start_prepared_event_combat(super::events::lantern_key::ENCOUNTER)?;
                }
            }
            return Ok(());
        }
        if event.id == EventId::WhisperingHollow {
            self.unported = None;
            if option_id.is(EventOptionId::WhisperingHollowInitialGold) {
                // WhisperingHollow.Gold / PotionReward.Populate: two separate
                // factory calls (duplicates allowed), owner's Rewards stream.
                self.current_event = None;
                self.apply_event_effect(EventEffect::LoseGold(event.ints[0]))?;
                self.reward = RewardState::default();
                for _ in 0..2 {
                    let Some(id) = self.roll_potion() else { return Ok(()); };
                    self.reward.items.push(RewardItem {
                        kind: RewardItemKind::Potion(id), consumed: false, skippable: true,
                        skip_completes: true, add_injury_after: false, neows_bones_final: false,
                    });
                }
                self.reward.active = None; // OfferCustom shows the rewards container.
                self.reward.stay_in_container = true;
                self.phase = RunPhase::CardReward;
            } else {
                // WhisperingHollow.Hug transforms with base.Rng before Damage.
                let candidates = self.transformable_deck_indices();
                if candidates.len() <= 1 {
                    for index in candidates {
                        let id = self.run_ctx().deck[index].instance_id;
                        self.transform_deck_card_with_rng(id, &mut event.rng)?;
                    }
                    self.apply_event_effect(EventEffect::LoseHp(9))?;
                    self.phase = if self.run_ctx().hp == 0 { RunPhase::RunOver } else { RunPhase::MapChoice };
                } else {
                    self.current_event = Some(event);
                    self.begin_run_card_choice("event.whispering_hollow.hug", "Choose a card to transform", &candidates, 1, 1, false,
                        RunChoiceEffect::TransformEventRngAndDamage(9), ChoiceResume::MapChoice);
                }
            }
            return Ok(());
        }
        if event.id == EventId::JungleMazeAdventure {
            self.unported = None;
            let solo = option_id.is(EventOptionId::JungleMazeAdventureInitialSoloQuest);
            if solo {
                // JungleMazeAdventure.cs:74: StableShuffle consumes two draws
                // even though its three shuffled VFX have no gameplay effect.
                event.rng.shuffle(&mut [0, 1, 2]);
                self.apply_event_effect(EventEffect::LoseHp(18))?;
            }
            if self.run_ctx().hp > 0 && self.unported.is_none() {
                self.apply_event_effect(EventEffect::GainGold(event.ints[if solo { 0 } else { 1 }]))?;
            }
            self.current_event = None;
            self.phase = if self.run_ctx().hp == 0 { RunPhase::RunOver } else { RunPhase::MapChoice };
            return Ok(());
        }
        if event.id == EventId::Pael {
            return self.choose_pael(event, option_id);
        }
        if event.id == EventId::SapphireSeed {
            // C# SapphireSeed.Eat/Plant: heal before the upgrade selector;
            // Sown uses the ordinary CanEnchant predicate and no random choice.
            self.current_event = None;
            self.unported = None;
            self.phase = RunPhase::MapChoice;
            let (candidates, prompt, effect) = if option_id.is(EventOptionId::SapphireSeedInitialEat) {
                self.apply_event_effect(EventEffect::Heal(9))?;
                (self.upgradable_deck_indices(), "Choose 1 cards to upgrade", RunChoiceEffect::Upgrade)
            } else {
                let candidates = self.run_ctx().deck.iter().enumerate()
                    .filter_map(|(i,c)| Self::event_can_enchant(c, "Sown").then_some(i)).collect();
                (candidates, "Choose a card to enchant with Sown",
                    RunChoiceEffect::Enchant { name: EnchantmentId::Sown, amount: 1 })
            };
            if !candidates.is_empty() {
                self.begin_run_card_choice("event.sapphire_seed.select", prompt, &candidates, 1, 1, false,
                    effect, ChoiceResume::MapChoice);
                if candidates.len() == 1 { self.resolve_run_choice(0)?; }
            }
            return Ok(());
        }
        if event.id == EventId::TabletOfTruth {
            // C# TabletOfTruth.Decipher / GetDecipherCost / LoseMaxHpAndUpgrade.
            if option_id.is(EventOptionId::TabletOfTruthInitialSmash)
                || option_id.is(EventOptionId::TabletOfTruthDecipherGiveUp) {
                if option_id.is(EventOptionId::TabletOfTruthInitialSmash) { self.apply_event_effect(EventEffect::Heal(20))?; }
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            let cost = event.ints[1];
            if cost >= self.run_ctx().max_hp {
                if self.has_event_death_prevention() {
                    let reason = "TabletOfTruth lethal Kill death prevention is unported";
                    self.unported = Some(reason);
                    return Err(reason);
                }
                self.apply_event_effect(EventEffect::LoseMaxHp(self.run_ctx().max_hp - 1))?;
                self.run_ctx_mut().hp = 0;
                self.phase = RunPhase::RunOver; // Simulator terminal-death convention.
                return Ok(());
            }
            self.apply_event_effect(EventEffect::LoseMaxHp(cost))?;
            let candidates = self.upgradable_deck_indices();
            if event.ints[0] == 4 {
                for index in candidates { self.run_ctx_mut().upgrade_deck_card(index); }
            } else if !candidates.is_empty() {
                let index = candidates[event.rng.choice_index(candidates.len())];
                self.run_ctx_mut().upgrade_deck_card(index);
            }
            event.ints[0] += 1;
            if event.ints[0] == 5 {
                self.phase = RunPhase::MapChoice;
            } else {
                event.ints[1] = match event.ints[0] {
                    1 => 6, 2 => 12, 3 => 24, 4 => self.run_ctx().max_hp - 1,
                    _ => unreachable!(),
                };
                event.options.clear();
                event.options.set_page(crate::identity::EventPageId::TabletOfTruthDecipherDecipherCount,
                    super::events::native_identity::EventParameter::Counter(event.ints[0]));
                event.options.push(super::events::EventOption::new("decipher", "Decipher", "")
                    .with_dynamic_description(EventDescription::TabletDecipherCost(event.ints[1])));
                event.options.push(super::events::EventOption::new("give_up", "Give Up", "Stop deciphering"));
                self.current_event = Some(event);
                self.phase = RunPhase::Event;
            }
            return Ok(());
        }
        if event.id == EventId::SelfHelpBook {
            self.current_event = None;
            self.unported = None;
            if option_id.is(EventOptionId::SelfHelpBookInitialNoOptions) {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            // C# SelfHelpBook.SelectAndEnchant: exactly one, restricted by type.
            let (ty, enchantment, prompt) = match option_id.id {
                EventOptionId::SelfHelpBookInitialReadTheBack => (CardType::Attack, "Sharp", "Choose 1 cards to enchant with Sharp"),
                EventOptionId::SelfHelpBookInitialReadPassage => (CardType::Skill, "Nimble", "Choose a card to enchant with Nimble"),
                EventOptionId::SelfHelpBookInitialReadEntireBook => (CardType::Power, "Swift", "Choose a card to enchant with Swift"),
                _ => return Err("invalid book option"),
            };
            let candidates: Vec<_> = self.run_ctx().deck.iter().enumerate()
                .filter_map(|(i,c)| (c.card_type == ty && Self::event_can_enchant(c, enchantment)).then_some(i))
                .collect();
            // CardSelectCmd.FromDeckForEnchantment bypasses the UI when there
            // is only one eligible card and manual confirmation is not required.
            if candidates.len() <= 1 {
                if let Some(&index) = candidates.first() {
                    if !crate::card::enchant::apply_static_enchantment(
                        &mut self.run_ctx_mut().deck[index], enchantment, 2)
                    { return Err("SelfHelpBook enchantment capacity exceeded"); }
                }
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice("event.self_help_book.enchant", prompt, &candidates, 1, 1, false,
                RunChoiceEffect::Enchant { name: EnchantmentId::from_name(enchantment).expect("book enchantment"), amount: 2 }, ChoiceResume::MapChoice);
            return Ok(());
        }
        if event.id == EventId::BrainLeech {
            self.current_event = None;
            self.unported = None;
            // C# BrainLeech.Rip: Damage then one colorless CardReward,
            // NoUpgradeRoll and NoCardPoolModifications.
            if option_id.is(EventOptionId::BrainLeechInitialRip) {
                self.apply_event_effect(EventEffect::LoseHp(5))?;
                if self.run_ctx().hp == 0 { return Ok(()); }
                let item = self.make_noncombat_card_reward(3, CardPool::Colorless, None, false, true, false)?;
                self.reward = RewardState::default();
                self.reward.items.push(item);
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
            } else {
                // C# ShareKnowledge: five distinct character cards with default
                // base odds, NoUpgradeRoll, noncancelable one-card grid.
                let mut cards = Vec::new();
                let mut chosen = Vec::new();
                for _ in 0..5 {
                    let rarity = self.roll_base_card_rarity();
                    let card = self.generate_noncombat_card(self.character.into(), rarity, &chosen, false)?;
                    chosen.push(card.id);
                    cards.push(card);
                }
                for chunk in cards.chunks_mut(3) {
                    let mut options: ArrayVec<Card, 4> = chunk.iter().cloned().collect();
                    self.apply_card_reward_relic_hooks(&mut options, false);
                    chunk.clone_from_slice(&options);
                }
                self.offer_card_grid("event:BRAIN_LEECH", cards, 1, 1, false, "Choose a card to add to your deck");
                self.pending_run_choice.as_mut().unwrap().single_select = true;
            }
            return Ok(());
        }
        if event.id == EventId::LuminousChoir {
            self.current_event = None;
            self.unported = None;
            self.phase = RunPhase::MapChoice;
            // C# LuminousChoir.ReachIntoTheFlesh removes before adding the curse;
            // OfferTribute spends the event's retained price before pulling a relic.
            if option_id.is(EventOptionId::LuminousChoirInitialReachIntoTheFlesh) {
                let curse = CardId::from_name("SPORE_MIND").ok_or("SporeMind missing")?;
                let candidates = self.removable_deck_indices();
                let count = candidates.len().min(2);
                if count == 0 {
                    self.add_card_to_deck(curse)?;
                } else if candidates.len() <= 2 {
                    // C# CardSelectCmd.FromDeckGeneric returns the entire list
                    // without opening UI when available count <= MinSelect.
                    let selected: Vec<_> = candidates.iter().map(|&i| self.run_ctx().deck[i].instance_id).collect();
                    self.run_ctx_mut().retain_run_deck_cards(|c| !selected.contains(&c.instance_id));
                    self.add_card_to_deck(curse)?;
                } else {
                    self.begin_run_card_choice("event.luminous_choir.remove", "Choose 2 cards to remove", &candidates,
                        count, count, false, RunChoiceEffect::RemoveAndAddCard(curse), ChoiceResume::MapChoice);
                    // CardSelectorPrefs(prompt, 2) has no manual-confirm flag.
                }
            } else {
                self.apply_event_effect(EventEffect::LoseGold(event.ints[0]))?;
                let relic = self.pull_player_relic(None, false);
                self.obtain_event_relic(relic)?;
            }
            return Ok(());
        }
        if event.id == EventId::RoomFullOfCheese {
            self.current_event = None;
            if option_id.is(EventOptionId::RoomFullOfCheeseInitialGorge) {
                let mut cards = self.generate_uniform_cards(8, self.character.into(), CardRarity::Common, false, false)?;
                // Frozen r7 RoomFullOfCheese creates one CardReward whose
                // is_card_reward_creation flag remains true even though its
                // card_creation_source is OTHER. Apply late reward listeners
                // once to the complete 8-card grid so stateful listeners such
                // as Silver Crucible spend one use and affect every option.
                self.apply_card_reward_relic_hooks_slice(&mut cards, true);
                self.offer_card_grid("event:ROOM_FULL_OF_CHEESE", cards, 2, 2, false, "Choose 2 cards");
            } else {
                self.apply_event_effect(EventEffect::LoseHp(14))?;
                // Simulator terminal-death convention, not a C# Search guard:
                // RoomFullOfCheese.Search continues to RelicCmd.Obtain after Damage.
                if self.run_ctx().hp == 0 {
                    return Ok(());
                }
                self.obtain_event_relic(RelicId::ChosenCheese)?;
                self.phase = if self.run_ctx().hp <= 0 { RunPhase::RunOver } else { RunPhase::MapChoice };
            }
            return Ok(());
        }
        if event.id == EventId::ThisOrThat && option_id.is(EventOptionId::ThisOrThatInitialOrnate) {
            // C# ThisOrThat.Ornate obtains RelicFactory's front-of-bag reward
            // (PlayerRng.Rewards rarity roll), then adds Clumsy, in that order.
            let relic = self.pull_player_relic(None, false);
            self.obtain_event_relic(relic)?;
            if self.unported.is_some() {
                self.current_event = Some(event);
                return Ok(());
            }
            self.add_card_to_deck(CardId::Clumsy)?;
            self.current_event = None;
            self.unported = None;
            self.phase = RunPhase::MapChoice;
            return Ok(());
        }
        if event.id == EventId::Amalgamator
            && matches!(option_id.id,
                EventOptionId::AmalgamatorInitialCombineStrikes |
                EventOptionId::AmalgamatorInitialCombineDefends)
        {
            let (tag, target, prompt) = if option_id.is(EventOptionId::AmalgamatorInitialCombineStrikes) {
                (
                    CardTag::Strike,
                    CardId::UltimateStrike,
                    "Choose 2 Strikes to combine",
                )
            } else {
                (
                    CardTag::Defend,
                    CardId::UltimateDefend,
                    "Choose 2 Defends to combine",
                )
            };
            let candidates: Vec<usize> = self
                .run_ctx()
                .deck
                .iter()
                .enumerate()
                .filter_map(|(index, card)| {
                    (card.rarity == CardRarity::Basic
                        && card.id != CardId::AscendersBane
                        && card.card_type != CardType::Quest
                        && !card.has_kw(KW_ETERNAL)
                        && card.has_tag(tag))
                    .then_some(index)
                })
                .collect();
            self.current_event = None;
            self.unported = None;
            if candidates.len() < 2 {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.amalgamator.select",
                prompt,
                &candidates,
                2,
                2,
                false,
                RunChoiceEffect::RemoveAndAddCard(target),
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::AromaOfChaos {
            let let_go = option_id.is(EventOptionId::AromaOfChaosInitialLetGo);
            let (candidates, prompt, effect) = if let_go {
                (
                    self.transformable_deck_indices(),
                    "Choose a card to transform",
                    RunChoiceEffect::TransformEventRng,
                )
            } else {
                (
                    self.upgradable_deck_indices(),
                    "Choose a card to upgrade",
                    RunChoiceEffect::Upgrade,
                )
            };
            self.unported = None;
            if candidates.is_empty() {
                self.current_event = None;
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.current_event = let_go.then_some(event);
            self.begin_run_card_choice(
                "event.aroma_of_chaos.select",
                prompt,
                &candidates,
                1,
                1,
                false,
                effect,
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::DoorsOfLightAndDark && option_id.is(EventOptionId::DoorsOfLightAndDarkInitialLight) {
            self.upgrade_random_event_cards(&mut event.rng, 2);
            self.current_event = None;
            self.unported = None;
            self.phase = RunPhase::MapChoice;
            return Ok(());
        }
        if event.id == EventId::DoorsOfLightAndDark && option_id.is(EventOptionId::DoorsOfLightAndDarkInitialDark) {
            let candidates = self.removable_deck_indices();
            self.current_event = None;
            self.unported = None;
            if candidates.is_empty() {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.doors_of_light_and_dark.remove",
                "Choose 1 card to remove",
                &candidates,
                1,
                1,
                false,
                RunChoiceEffect::Remove { damage_after: 0 },
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::DrowningBeacon && option_id.is(EventOptionId::DrowningBeaconInitialBottle) {
            let potion = PotionId::Glowwaterpotion;
            self.current_event = None;
            self.unported = None;
            self.reward = RewardState::default();
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Potion(potion),
                consumed: false,
                skippable: true,
                skip_completes: true,
                add_injury_after: false,
                neows_bones_final: false,
            });
            self.reward.active = Some(0);
            self.phase = RunPhase::CardReward;
            return Ok(());
        }
        if event.id == EventId::GraveOfTheForgotten && option_id.is(EventOptionId::GraveOfTheForgottenInitialConfront) {
            self.add_card_to_deck(CardId::Decay)?;
            let candidates: Vec<usize> = self
                .run_ctx()
                .deck
                .iter()
                .enumerate()
                .filter_map(|(index, card)| Self::event_can_enchant(card, "SoulsPower").then_some(index))
                .collect();
            self.current_event = None;
            self.unported = None;
            if candidates.is_empty() {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.grave_of_the_forgotten.enchant",
                "Choose a card to enchant with Soul's Power",
                &candidates,
                1,
                1,
                false,
                RunChoiceEffect::Enchant {
                    name: EnchantmentId::SoulsPower,
                    amount: 1,
                },
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::MorphicGrove && option_id.is(EventOptionId::MorphicGroveInitialGroup) {
            self.run_ctx_mut().gold = 0;
            let candidates = self.transformable_deck_indices();
            let count = candidates.len().min(2);
            self.unported = None;
            if count == 0 {
                self.current_event = None;
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.current_event = Some(event);
            self.begin_run_card_choice(
                "event.morphic_grove.transform",
                "Choose 2 cards to transform",
                &candidates,
                count,
                count,
                false,
                RunChoiceEffect::TransformEventRng,
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::PotionCourier {
            let mut potions = Vec::new();
            if option_id.is(EventOptionId::PotionCourierInitialGrabPotions) {
                let foul = PotionId::Foulpotion;
                potions.extend([foul; 3]);
            } else if let Some(potion) = self.roll_event_potion_rarity(PotionRarity::Uncommon) {
                potions.push(potion);
            }
            self.current_event = None;
            self.unported = None;
            self.reward = RewardState::default();
            for potion in potions {
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Potion(potion),
                    consumed: false,
                    skippable: true,
                    skip_completes: true,
                    add_injury_after: false,
                    neows_bones_final: false,
                });
            }
            if self.reward.items.is_empty() {
                self.phase = RunPhase::MapChoice;
            } else {
                self.reward.active = if self.reward.items.len() == 1 { Some(0) } else { None };
                self.phase = RunPhase::CardReward;
            }
            return Ok(());
        }
        if event.id == EventId::SpiralingWhirlpool && option_id.is(EventOptionId::SpiralingWhirlpoolInitialObserve) {
            let candidates: Vec<usize> = self
                .run_ctx()
                .deck
                .iter()
                .enumerate()
                .filter_map(|(index, card)| Self::event_can_enchant(card, "Spiral").then_some(index))
                .collect();
            self.current_event = None;
            self.unported = None;
            if candidates.is_empty() {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.spiraling_whirlpool.enchant",
                "Choose a card to enchant with Spiral",
                &candidates,
                1,
                1,
                false,
                RunChoiceEffect::Enchant {
                    name: EnchantmentId::Spiral,
                    amount: 1,
                },
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::Symbiote {
            let (candidates, prompt, effect): (Vec<usize>, &'static str, RunChoiceEffect) =
                if option_id.is(EventOptionId::SymbioteInitialApproach) {
                (
                    self.run_ctx()
                        .deck
                        .iter()
                        .enumerate()
                        .filter_map(|(index, card)| {
                            Self::event_can_enchant(card, "Corrupted").then_some(index)
                        })
                        .collect(),
                    "Choose a card to enchant with Corrupted",
                    RunChoiceEffect::Enchant {
                        name: EnchantmentId::Corrupted,
                        amount: 1,
                    },
                )
            } else {
                (
                    self.transformable_deck_indices(),
                    "Choose a card to transform",
                    RunChoiceEffect::TransformEventRng,
                )
            };
            self.unported = None;
            if candidates.is_empty() {
                self.current_event = None;
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.current_event = option_id.is(EventOptionId::SymbioteInitialKillWithFire).then_some(event);
            self.begin_run_card_choice(
                "event.symbiote.select",
                prompt,
                &candidates,
                1,
                1,
                false,
                effect,
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::WoodCarvings {
            let (candidates, prompt, effect): (Vec<usize>, &'static str, RunChoiceEffect) =
                if option_id.is(EventOptionId::WoodCarvingsInitialSnake) {
                (
                    self.run_ctx()
                        .deck
                        .iter()
                        .enumerate()
                        .filter_map(|(index, card)| {
                            Self::event_can_enchant(card, "Slither").then_some(index)
                        })
                        .collect(),
                    "Choose a card to enchant with Slither",
                    RunChoiceEffect::Enchant {
                        name: EnchantmentId::Slither,
                        amount: 1,
                    },
                )
            } else {
                let bird = option_id.is(EventOptionId::WoodCarvingsInitialBird);
                let target = if bird {
                    CardId::Peck
                } else {
                    CardId::ToricToughness
                };
                (
                    self.run_ctx()
                        .deck
                        .iter()
                        .enumerate()
                        .filter_map(|(index, card)| {
                            (card.rarity == CardRarity::Basic
                                && card.id != CardId::AscendersBane
                                && card.card_type != CardType::Quest
                                && !card.has_kw(KW_ETERNAL))
                            .then_some(index)
                        })
                        .collect(),
                    if bird {
                        "Choose a Basic card to transform into Peck"
                    } else {
                        "Choose a Basic card to transform into Toric Toughness"
                    },
                    RunChoiceEffect::RemoveAndAddCard(target),
                )
            };
            self.current_event = None;
            self.unported = None;
            if candidates.is_empty() {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.wood_carvings.enchant",
                prompt,
                &candidates,
                1,
                1,
                false,
                effect,
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::ZenWeaver && matches!(option_id.id,
            EventOptionId::ZenWeaverInitialEmotionalAwareness |
            EventOptionId::ZenWeaverInitialArachnidAcupuncture) {
            let candidates = self.removable_deck_indices();
            let (wanted, gold, prompt) = if option_id.is(EventOptionId::ZenWeaverInitialEmotionalAwareness) {
                (1, 125, "Choose 1 card to remove")
            } else {
                (2, 250, "Choose 2 cards to remove")
            };
            let count = wanted.min(candidates.len());
            self.current_event = None;
            self.unported = None;
            if count == 0 {
                self.run_ctx_mut().gold = (self.run_ctx().gold - gold).max(0);
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.zen_weaver.remove",
                prompt,
                &candidates,
                count,
                count,
                false,
                RunChoiceEffect::RemoveAndLoseGold(gold),
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::WaterloggedScriptorium
            && matches!(option_id.id,
                EventOptionId::WaterloggedScriptoriumInitialTentacleQuill |
                EventOptionId::WaterloggedScriptoriumInitialPricklySponge)
        {
            let quill = option_id.is(EventOptionId::WaterloggedScriptoriumInitialTentacleQuill);
            let cost = if quill {
                55
            } else {
                99
            };
            let required = if quill { 1 } else { 2 };
            self.run_ctx_mut().gold -= cost;
            let candidates: Vec<usize> = self
                .run_ctx()
                .deck
                .iter()
                .enumerate()
                .filter_map(|(index, card)| {
                    Self::event_can_enchant(card, "Steady").then_some(index)
                })
                .collect();
            let count = required.min(candidates.len());
            self.current_event = None;
            self.unported = None;
            if count == 0 {
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event:WATERLOGGED_SCRIPTORIUM",
                if count == 1 {
                    "Choose a card to enchant with Steady"
                } else {
                    "Choose 2 cards to enchant with Steady"
                },
                &candidates,
                count,
                count,
                false,
                RunChoiceEffect::Enchant {
                    name: EnchantmentId::Steady,
                    amount: 1,
                },
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::DenseVegetation && option_id.is(EventOptionId::DenseVegetationRestFight) {
            // C# DenseVegetation.Fight passes empty EXTRA rewards; the ordinary
            // Monster room still generates its gold/potion/card rewards.
            self.current_event = None;
            self.unported = None;
            return self.enter_encounter(RoomType::Monster, EncounterId::DenseVegetationEvent);
        }
        if event.id == EventId::Wellspring && option_id.is(EventOptionId::WellspringInitialBathe) {
            let candidates = self.removable_deck_indices();
            let guilty = CardId::from_name("GUILTY").ok_or("Guilty card missing")?;
            self.current_event = None;
            self.unported = None;
            if candidates.is_empty() {
                self.add_card_to_deck(guilty)?;
                self.phase = RunPhase::MapChoice;
                return Ok(());
            }
            self.begin_run_card_choice(
                "event.wellspring.remove",
                "Choose 1 card to remove",
                &candidates,
                1,
                1,
                false,
                RunChoiceEffect::RemoveAndAddCard(guilty),
                ChoiceResume::MapChoice,
            );
            return Ok(());
        }
        if event.id == EventId::Wellspring && option_id.is(EventOptionId::WellspringInitialBottle) {
            let potion = self.roll_event_potion();
            self.current_event = None;
            self.unported = None;
            self.reward = RewardState::default();
            if let Some(potion) = potion {
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Potion(potion),
                    consumed: false,
                    skippable: true,
                    skip_completes: true,
                    add_injury_after: false,
                    neows_bones_final: false,
                });
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
            } else {
                self.phase = RunPhase::MapChoice;
            }
            return Ok(());
        }
        if event.id == EventId::TheLegendsWereTrue && option_id.is(EventOptionId::TheLegendsWereTrueInitialSlowlyFindAnExit) {
            let resolution =
                super::events::choose(&mut event, option_id).ok_or("event option unported")?;
            for effect in resolution.effects {
                self.apply_event_effect(effect)?;
            }
            self.current_event = None;
            self.unported = None;
            if self.run_ctx().hp <= 0 {
                self.phase = RunPhase::RunOver;
                return Ok(());
            }
            self.reward = RewardState::default();
            if let Some(potion) = self.roll_event_potion() {
                self.reward.items.push(RewardItem {
                    kind: RewardItemKind::Potion(potion),
                    consumed: false,
                    skippable: true,
                    skip_completes: true,
                    add_injury_after: false,
                    neows_bones_final: false,
                });
                self.reward.active = Some(0);
                self.phase = RunPhase::CardReward;
            } else {
                self.phase = RunPhase::MapChoice;
            }
            return Ok(());
        }
        if event.id == EventId::UnrestSite && option_id.is(EventOptionId::UnrestSiteInitialKill) {
            let resolution =
                super::events::choose(&mut event, option_id).ok_or("event option unported")?;
            for effect in resolution.effects {
                self.apply_event_effect(effect)?;
            }
            // C# UnrestSite.Kill obtains directly; there is no skippable reward modal.
            let relic = self.pull_player_relic(None, false);
            self.obtain_event_relic(relic)?;
            self.current_event = None;
            if self.unported.is_none() {
                self.phase = RunPhase::MapChoice;
            }
            return Ok(());
        }
        let resolution =
            super::events::choose(&mut event, option_id).ok_or("event option unported")?;
        for effect in resolution.effects {
            // C# CreatureCmd.Damage aborts the remaining event after fatal damage.
            if self.run_ctx().hp <= 0 || self.unported.is_some() {
                break;
            }
            if effect == EventEffect::RollTrashRelic {
                let id = super::events::roll_trash_relic(&mut event);
                self.obtain_event_relic(id)?;
                continue;
            }
            self.apply_event_effect(effect)?;
        }
        if self.run_ctx().hp <= 0 {
            self.current_event = None;
            self.phase = RunPhase::RunOver;
            self.unported = None;
            return Ok(());
        }
        if self.unported.is_some() {
            self.current_event = Some(event);
            return Ok(());
        }
        if resolution.finished {
            self.current_event = None;
            self.phase = RunPhase::MapChoice;
            self.unported = None;
        } else {
            event.refresh_public_page(self.run_ctx().hp);
            event.options.replace(resolution.next_options);
            self.current_event = Some(event);
        }
        Ok(())
    }

    fn move_to(&mut self, coord: Coord) -> Result<(), &'static str> {
        if self.phase != RunPhase::MapChoice {
            return Err("not at map choice");
        }
        let free_travel = self.winged_boots_free_travel();
        let (dest, point_type, child_all_shop, used_flight) = {
            let map = self.acts[self.current_act].map.as_ref().ok_or("no map")?;
            let dest = map.point_index(coord).ok_or("unknown map point")?;
            let reachable = if self.visited_points.is_empty() {
                dest == map.start
            } else if free_travel {
                let row = map.points[self.current_point as usize].coord.row + 1;
                map.points[dest as usize].coord.row == row
            } else {
                map.points[self.current_point as usize]
                    .children
                    .contains(&dest)
            };
            if !reachable {
                return Err("unreachable map point");
            }
            let point = &map.points[dest as usize];
            let child_all_shop = !point.children.is_empty()
                && point
                    .children
                    .iter()
                    .all(|child| map.points[*child as usize].point_type == MapPointType::Shop);
            let used_flight = !self.visited_points.is_empty()
                && free_travel
                && !map.points[self.current_point as usize]
                    .children
                    .contains(&dest);
            (dest, point.point_type, child_all_shop, used_flight)
        };

        // run: run/run_manager.py:1685
        let room = match point_type {
            MapPointType::Monster => RoomType::Monster,
            MapPointType::Elite => RoomType::Elite,
            MapPointType::Boss => RoomType::Boss,
            MapPointType::Shop => RoomType::Shop,
            MapPointType::RestSite => RoomType::RestSite,
            MapPointType::Treasure => RoomType::Treasure,
            MapPointType::Ancient => RoomType::Event,
            MapPointType::Unknown => self.resolve_unknown_room(child_all_shop),
            MapPointType::Unassigned => return Err("unassigned map point"),
        };

        if used_flight {
            // run: relics/ancient_new.py:361
            if let Some(relic) = self
                .run_ctx_mut()
                .owned_relics_mut(crate::combat::PLAYER)
                .find(|relic| relic.id == RelicId::WingedBoots)
            {
                relic.counter += 1;
                if relic.counter >= 3 {
                    relic.enabled = false;
                }
            }
        }
        self.current_point = dest;
        self.visited_points.push(dest);
        self.act_floor = coord.row + 1;
        let run = self.run_ctx_mut();
        run.total_floor += 1;
        run.coord = Some((coord.col, coord.row));
        self.run_ctx_mut().room_type = room;
        // run: run/run_state.py:1905
        self.map_history.push(MapHistoryEntry {
            map_point_type: point_type,
            room_type: room,
        });
        match room {
            RoomType::Monster | RoomType::Elite | RoomType::Boss => {
                rest::after_room_entered(self.run_ctx_mut(), room, point_type);
                self.enter_combat(room)
            }
            RoomType::Event => {
                rest::after_room_entered(self.run_ctx_mut(), room, point_type);
                self.enter_event_room(point_type == MapPointType::Ancient)
            }
            RoomType::Shop => self.enter_shop(point_type),
            RoomType::RestSite => {
                rest::after_room_entered(self.run_ctx_mut(), room, point_type);
                self.enter_rest_site()
            }
            RoomType::Treasure => {
                rest::after_room_entered(self.run_ctx_mut(), room, point_type);
                self.enter_treasure()
            }
        }
    }

    fn enter_shop(&mut self, point_type: MapPointType) -> Result<(), &'static str> {
        self.run_choice = None;
        let rarity_offset = self.card_rarity_offset as f64;
        let removals_used = self.shop_removals_used;
        let multiplayer = self.is_multiplayer();
        {
            let run = if let Some(combat) = self.combat.as_mut() {
                &mut combat.run
            } else {
                self.run.as_mut().expect("run context")
            };
            shop::generate(
                &mut self.shop,
                run,
                self.character,
                multiplayer,
                &mut self.relic_bags,
                rarity_offset,
                removals_used,
            )?;
            rest::after_room_entered(run, RoomType::Shop, point_type);
        }
        self.phase = RunPhase::Shop;
        self.unported = None;
        if self
            .run_ctx()
            .owned_relics(crate::combat::PLAYER)
            .any(|relic| relic.id == RelicId::LordsParasol && relic.enabled)
        {
            self.auto_purchase_shop_inventory()?;
        }
        Ok(())
    }

    fn auto_purchase_shop_inventory(&mut self) -> Result<(), &'static str> {
        // LordsParasol iterates the original slots once. Courier may refill a
        // purchased slot, but the refill is not purchased again on this entry.
        let start=self.parasol_next_relic.take().unwrap_or(0);
        let card_count = if start==0 {self.shop.cards.len() + self.shop.colorless_cards.len()}else{0};
        for index in 0..card_count {
            let Some(mut card) = self
                .shop
                .cards
                .iter()
                .chain(self.shop.colorless_cards.iter())
                .nth(index)
                .and_then(|entry| entry.card.clone())
            else {
                continue;
            };
            {
                let run = self.run_ctx_mut();
                card.instance_id = run.new_card_id();
                run.deck.push(card);
                run.after_run_deck_entry(run.deck.len() - 1);
            }
            let refill = shop::post_purchase(self.run_ctx_mut(), 0);
            if refill {
                let rarity_offset = self.card_rarity_offset as f64;
                let run = if let Some(combat) = self.combat.as_mut() {
                    &mut combat.run
                } else {
                    self.run.as_mut().expect("run context")
                };
                shop::refill_card(&mut self.shop, index, run, rarity_offset)?;
            } else if index < self.shop.cards.len() {
                self.shop.cards[index].card = None;
                self.shop.cards[index].price = shop::SOLD_OUT;
            } else {
                let colorless = index - self.shop.cards.len();
                self.shop.colorless_cards[colorless].card = None;
                self.shop.colorless_cards[colorless].price = shop::SOLD_OUT;
            }
        }

        let relic_count = self.shop.relics.len();
        for index in start..relic_count {
            let entry = self.shop.relics[index];
            if entry.price >= shop::SOLD_OUT {
                continue;
            }
            self.remove_relic_from_bags(entry.relic_id)?;
            let pickup = shop::add_relic(self.run_ctx_mut(), entry.relic_id);
            let refill = shop::post_purchase(self.run_ctx_mut(), 0);
            if refill {
                let run = if let Some(combat) = self.combat.as_mut() {
                    &mut combat.run
                } else {
                    self.run.as_mut().expect("run context")
                };
                shop::refill_relic(&mut self.shop, index, run, &mut self.relic_bags)?;
            } else {
                self.shop.relics[index].price = shop::SOLD_OUT;
            }
            if self.offer_nr_pickup(entry.relic_id)? {
                if self.pending_run_choice.is_some()||self.phase==RunPhase::CardReward {
                    self.parasol_next_relic=Some(index+1);
                    return Ok(());
                }
            } else if let Err(reason) = pickup {
                self.unported.get_or_insert(reason);
            }
        }

        let potion_count = self.shop.potions.len();
        for index in 0..potion_count {
            let entry = self.shop.potions[index];
            if entry.price >= shop::SOLD_OUT {
                continue;
            }
            if !shop::procure_potion(self.run_ctx_mut(), entry.potion_id) {
                continue;
            }
            let refill = shop::post_purchase(self.run_ctx_mut(), 0);
            if refill {
                let run = if let Some(combat) = self.combat.as_mut() {
                    &mut combat.run
                } else {
                    self.run.as_mut().expect("run context")
                };
                shop::refill_potion(&mut self.shop, index, run)?;
            } else {
                self.shop.potions[index].price = shop::SOLD_OUT;
            }
        }

        if !self.shop.removal_used && self.run_choice.is_none() {
            let candidates = rest::removable_indices(self.run_ctx());
            if !candidates.is_empty() {
                self.run_choice = Some(RunDeckChoice {
                    kind: RunDeckChoiceKind::ShopRemove { gold_spent: 0 },
                    candidates,
                    selected: Vec::new(),
                });
            }
        }
        Ok(())
    }

    fn enter_rest_site(&mut self) -> Result<(), &'static str> {
        self.run_choice = None;
        let run = if let Some(combat) = self.combat.as_mut() {
            &mut combat.run
        } else {
            self.run.as_mut().expect("run context")
        };
        rest::generate(&mut self.rest, run);
        self.phase = RunPhase::RestSite;
        self.unported = None;
        Ok(())
    }

    fn take_run_from_owner(&mut self) -> RunCtx {
        if let Some(old) = self.combat.take() {
            old.run
        } else {
            self.run.take().expect("run owner")
        }
    }

    fn enter_combat(&mut self, room: RoomType) -> Result<(), &'static str> {
        let encounter = {
            let act = &mut self.acts[self.current_act];
            match room {
                RoomType::Monster => {
                    let id = *act
                        .rooms
                        .normal
                        .get(act.normal_cursor)
                        .ok_or("normal encounter sequence exhausted")?;
                    act.normal_cursor += 1;
                    id
                }
                RoomType::Elite => {
                    let id = *act
                        .rooms
                        .elite
                        .get(act.elite_cursor)
                        .ok_or("elite encounter sequence exhausted")?;
                    act.elite_cursor += 1;
                    id
                }
                RoomType::Boss => {
                    let map = act.map.as_ref().unwrap();
                    if map.second_boss == Some(self.current_point) {
                        act.rooms.second_boss.unwrap_or(act.rooms.boss)
                    } else {
                        act.rooms.boss
                    }
                }
                _ => return Err("not a combat room"),
            }
        };
        self.enter_encounter(room, encounter)
    }

    fn enter_encounter(&mut self, room: RoomType, encounter: EncounterId) -> Result<(), &'static str> {
        self.encounter_id = Some(encounter);
        let combat_seed = self.manager_rng.next_int(0, INT_MAX) as u64;
        let run_seed = self.run_ctx().rngs.seed;
        let floor = self.run_ctx().total_floor;
        let mut run = self.take_run_from_owner();
        run.room_type = room;
        let mut combat = Combat::new_for_character(run, combat_seed, self.character);
        // C# CombatState retains each Player.Character; generated combat cards
        // (Metamorphosis, etc.) must use that owner's pool, not its deck colors.
        combat.set_character_pool(crate::combat::PLAYER, self.character)?;
        let encounter_seed = run_seed
            .wrapping_add(floor as u64)
            .wrapping_add(deterministic_hash64(encounter.name()));
        let mut enc_rng = Rng::new(encounter_seed);
        combat.setup_encounter_id(encounter, &mut enc_rng);
        combat.start_combat();
        self.combat = Some(combat);
        self.phase = RunPhase::Combat;
        Ok(())
    }

    fn after_combat_action(&mut self) -> Result<(), &'static str> {
        let (won, room) = {
            let Some(c) = self.combat.as_mut() else {
                return Err("no combat");
            };
            // Until a multiplayer run constructor supplies the persistent roster,
            // never infer run ownership from an externally extended combat.
            if c.players.len() != self.player_count || self.player_count != 1 {
                self.unported = Some("multiplayer run construction is unported");
                return Ok(());
            }
            if let Some(fault) = c.fault {
                self.unported = Some(fault);
                return Ok(());
            }
            if !c.is_over {
                return Ok(());
            }
            let hp = c.player().hp;
            let max_hp = c.player().max_hp;
            c.run.hp = hp;
            c.run.max_hp = max_hp;
            (c.won, c.run.room_type)
        };
        if !won {
            self.fake_merchant_loot.clear();
            self.lantern_key_reward = None;
            self.phase = RunPhase::RunOver;
            return Ok(());
        }
        // Combat::end_combat already fires Guilty.AfterCombatEnd on run.deck.
        // Calling it again here would count each victory twice.
        if self.current_event.as_ref().is_some_and(|e| e.id == EventId::BattlewornDummy) {
            self.resume_battleworn_dummy()?;
        } else {
            self.generate_combat_rewards(room);
        }
        Ok(())
    }

    fn roll_card_rarity(&mut self, context: RoomType) -> CardRarity {
        let offset = if context == RoomType::Boss {
            0.0
        } else {
            self.card_rarity_offset
        };
        let (uncommon, rare) = self.card_rarity_thresholds(context);
        // C# CardRarityOdds compares float32 values.
        let roll = self.run_ctx_mut().rngs.rewards.next_float(1.0) as f32;
        let rare_threshold = rare + offset;
        let result = if roll < rare_threshold {
            CardRarity::Rare
        } else if roll < uncommon + rare_threshold {
            CardRarity::Uncommon
        } else {
            CardRarity::Common
        };
        // Encounter-source rolls update the future encounter rarity offset.
        if result == CardRarity::Rare {
            self.card_rarity_offset = -0.05;
        } else {
            self.card_rarity_offset = (self.card_rarity_offset
                + if self.options.ascension >= 7 { 0.005 } else { 0.01 }).min(0.40);
        }
        result
    }

    fn card_rarity_thresholds(&self, context: RoomType) -> (f32, f32) {
        if context == RoomType::Boss {
            (0.0, 1.0)
        } else if context == RoomType::Elite {
            if self.options.ascension >= 7 {
                (0.40, 0.05)
            } else {
                (0.40, 0.10)
            }
        } else if self.options.ascension >= 7 {
            (0.37, 0.0149)
        } else {
            (0.37, 0.03)
        }
    }

    fn reward_card_pool(&self, character: crate::character::CardPool) -> Result<Vec<CardId>, &'static str> {
        let mut pool = super::card_pools::cards(character, self.is_multiplayer())
            .map_err(|_| "card reward: unsupported character pool")?;
        // C# PrismaticGem.ModifyCardRewardCreationOptions unions unlocked pools.
        if character != crate::character::CardPool::Colorless && self.run_ctx().owned_relics(crate::combat::PLAYER)
            .any(|r| r.id == RelicId::PrismaticGem && r.enabled) {
            pool.clear();
            for character in orobas::CHARACTERS {
                pool.extend(super::card_pools::cards(character.into(), self.is_multiplayer())?);
            }
        }
        Ok(pool)
    }

    fn generate_reward_card(&mut self, rarity: CardRarity, chosen: &ArrayVec<CardId, 3>) -> Option<Card> {
        let mut pool = match self.reward_card_pool(self.character.into()) {
            Ok(pool) => pool, Err(reason) => { self.unported = Some(reason); return None; }
        };
        if let Err(reason) = self.add_dingy_rug_reward_pool(&mut pool, true) {
            self.unported = Some(reason); return None;
        }
        let candidates: Vec<CardId> = pool.into_iter()
            .filter(|id| card_info(*id, false).is_some_and(|info| info.rarity == rarity))
            .filter(|id| !chosen.contains(id))
            .collect();
        if candidates.is_empty() {
            self.unported = Some("card reward: empty eligible pool");
            return None;
        }
        let i = self
            .run_ctx_mut()
            .rngs
            .rewards
            .choice_index(candidates.len());
        let id = candidates[i];
        let upgrade_roll = self.run_ctx_mut().rngs.rewards.next_float(1.0);
        let scaling = if self.options.ascension >= 7 {
            0.125
        } else {
            0.25
        };
        let odds = if rarity == CardRarity::Rare {
            0.0
        } else {
            self.current_act as f64 * scaling
        };
        let upgraded = upgrade_roll <= odds;
        let iid = self.run_ctx_mut().new_card_id();
        let card = Card::from_factory(id, upgraded, iid);
        if card.is_none() {
            self.unported = Some("card reward: missing factory");
        }
        card
    }

    // sim_r1_events: CardReward generation with generation_context=None.
    fn roll_base_card_rarity(&mut self) -> CardRarity {
        self.roll_other_card_rarity(RoomType::Monster)
    }

    fn roll_other_card_rarity(&mut self, odds: RoomType) -> CardRarity {
        let roll = self.run_ctx_mut().rngs.rewards.next_float(1.0) as f32;
        let (uncommon, rare) = self.card_rarity_thresholds(odds);
        if roll < rare {
            CardRarity::Rare
        } else if roll < uncommon + rare {
            CardRarity::Uncommon
        } else {
            CardRarity::Common
        }
    }

    fn generate_noncombat_card(
        &mut self,
        character: crate::character::CardPool,
        rarity: CardRarity,
        chosen: &[CardId],
        roll_upgrade: bool,
    ) -> Result<Card, &'static str> {
        let pool = super::card_pools::cards(character, self.is_multiplayer())?;
        self.generate_card_from_pool(&pool, rarity, chosen, roll_upgrade)
    }

    fn generate_filtered_noncombat_card(
        &mut self,
        kind: Option<CardType>,
        cost: Option<i32>,
        costs_x: Option<bool>,
    ) -> Result<Card, &'static str> {
        let character = self.character;
        let multiplayer = self.is_multiplayer();
        let ascension = self.options.ascension;
        let id = super::events::infested_automaton::draw(character, multiplayer, ascension,
            &mut self.run_ctx_mut().rngs.rewards, kind, cost, costs_x)?;
        let iid = self.run_ctx_mut().new_card_id();
        let mut card = Card::from_factory(id, false, iid).ok_or("card factory missing")?;
        self.apply_card_reward_relic_hooks_slice(std::slice::from_mut(&mut card), false);
        Ok(card)
    }

    fn make_crystal_sphere_card_reward(
        &mut self,
        event_rng: &mut Rng,
        rarity: CardRarity,
    ) -> Result<RewardItem, &'static str> {
        let mut pool = self.reward_card_pool(self.character.into())?;
        self.add_dingy_rug_reward_pool(&mut pool, true)?;
        let mut chosen = Vec::new();
        let mut cards = ArrayVec::<Card, 4>::new();
        for _ in 0..3 {
            let candidates: Vec<CardId> = pool
                .iter()
                .copied()
                .filter(|id| !chosen.contains(id))
                .filter(|id| card_info(*id, false).is_some_and(|info| {
                    info.rarity == rarity
                        && !matches!(info.rarity, CardRarity::Basic | CardRarity::Ancient)
                }))
                .collect();
            if candidates.is_empty() {
                return Err("crystal sphere card reward pool exhausted");
            }
            let id = candidates[event_rng.choice_index(candidates.len())];
            chosen.push(id);
            let roll = event_rng.next_float(1.0);
            let scaling = if self.options.ascension >= 7 { 0.125 } else { 0.25 };
            let upgraded = roll <= if rarity == CardRarity::Rare {
                0.0
            } else {
                self.current_act as f64 * scaling
            };
            let iid = self.run_ctx_mut().new_card_id();
            cards.push(Card::from_factory(id, upgraded, iid).ok_or("card factory missing")?);
        }
        self.apply_card_reward_relic_hooks(&mut cards, true);
        Ok(RewardItem {
            kind: RewardItemKind::Card {
                cards,
                closed: false,
                reroll: Some(CardRewardRerollOptions::CrystalSphere(rarity)),
                progress: Default::default(),
            },
            consumed: false,
            skippable: true,
            skip_completes: false,
            add_injury_after: false,
            neows_bones_final: false,
        })
    }

    pub(super) fn generate_card_from_pool(&mut self, pool: &[CardId], rarity: CardRarity,
        chosen: &[CardId], roll_upgrade: bool) -> Result<Card, &'static str> {
        let mut rarity = rarity;
        let initial_rarity = rarity;
        // C# CardFactory.GetNextAllowedRarity: promote unavailable rarities,
        // wrapping through the reward rarities. Colorless has no Common cards.
        let candidates = loop {
            let candidates: Vec<CardId> = pool
            .iter()
            .copied()
            .filter(|id| !chosen.contains(id))
            .filter(|id| card_info(*id, false).is_some_and(|info| info.rarity == rarity))
            .collect();
            if !candidates.is_empty() { break candidates; }
            rarity = match rarity {
                CardRarity::Common => CardRarity::Uncommon,
                CardRarity::Uncommon => CardRarity::Rare,
                CardRarity::Rare => CardRarity::Common,
                _ => return Err("unsupported reward rarity"),
            };
            if rarity == initial_rarity { return Err("card pool exhausted"); }
        };
        let id = {
            let run = self.run_ctx_mut();
            candidates[run.rngs.rewards.choice_index(candidates.len())]
        };
        let upgraded = if roll_upgrade {
            let roll = self.run_ctx_mut().rngs.rewards.next_float(1.0);
            let scaling = if self.options.ascension >= 7 {
                0.125
            } else {
                0.25
            };
            // C# CardFactory.RollForUpgrade consumes the roll for every rarity,
            // but only non-Rare cards receive act scaling.
            roll <= if rarity == CardRarity::Rare { 0.0 } else { self.current_act as f64 * scaling }
        } else {
            false
        };
        let iid = self.run_ctx_mut().new_card_id();
        Card::from_factory(id, upgraded, iid).ok_or("card factory missing")
    }

    fn make_noncombat_card_reward(
        &mut self,
        count: usize,
        character: crate::character::CardPool,
        forced_rarity: Option<CardRarity>,
        roll_upgrade: bool,
        skip_completes: bool,
        add_injury_after: bool,
    ) -> Result<RewardItem, &'static str> {
        self.make_noncombat_card_reward_with_pool_modifications(
            count,
            character,
            forced_rarity,
            roll_upgrade,
            skip_completes,
            add_injury_after,
            true,
        )
    }

    fn make_noncombat_card_reward_with_pool_modifications(
        &mut self,
        count: usize,
        character: crate::character::CardPool,
        forced_rarity: Option<CardRarity>,
        roll_upgrade: bool,
        skip_completes: bool,
        add_injury_after: bool,
        modify_pool: bool,
    ) -> Result<RewardItem, &'static str> {
        let mut cards = ArrayVec::<Card, 4>::new();
        let mut chosen = Vec::new();
        // PrismaticGem.cs tests IsCardReward, not Source == Encounter.
        // This method constructs actual CardRewards; choose-a-card grids use
        // the separate unmodified-pool generation path.
        let pool = if modify_pool {
            self.reward_card_pool(character)?
        } else {
            super::card_pools::cards(character, self.is_multiplayer())?
        };
        for _ in 0..count.min(3) {
            let rarity = forced_rarity.unwrap_or_else(|| self.roll_base_card_rarity());
            let card = self.generate_card_from_pool(&pool, rarity, &chosen, roll_upgrade)?;
            chosen.push(card.id);
            cards.push(card);
        }
        self.apply_card_reward_relic_hooks(&mut cards, true);
        Ok(RewardItem {
            kind: RewardItemKind::Card {
                cards,
                closed: false,
                reroll: Some(CardRewardRerollOptions::Noncombat {
                    character: character.to_owned(), count: count.min(3),
                    rarity: forced_rarity, roll_upgrade, modify_pool,
                }),
                progress: Default::default(),
            },
            consumed: false,
            skippable: true,
            skip_completes,
            add_injury_after,
            neows_bones_final: false,
        })
    }

    fn add_dingy_rug_reward_pool(&self, pool: &mut Vec<CardId>, is_card_reward: bool) -> Result<(), &'static str> {
        // DingyRug.cs:13-27: only Owner's IsCardReward creation options union
        // Colorless, before rarity filtering. Grids (Cheese/SeaGlass/Tablet)
        // lack that flag; GlassEye and its retained reroll options have it.
        if is_card_reward && self.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::DingyRug) {
            for id in super::card_pools::cards(CardPool::Colorless, self.is_multiplayer())? {
                if !pool.contains(&id) { pool.push(id); }
            }
        }
        Ok(())
    }

    fn generate_uniform_cards(
        &mut self,
        count: usize,
        character: crate::character::CardPool,
        rarity: CardRarity,
        probe_candidates: bool,
        is_card_reward: bool,
    ) -> Result<Vec<Card>, &'static str> {
        self.generate_filtered_uniform_cards(count, character, rarity, probe_candidates,
            is_card_reward, None, true)
    }

    fn generate_filtered_uniform_cards(
        &mut self, count: usize, character: crate::character::CardPool, rarity: CardRarity,
        probe_candidates: bool, is_card_reward: bool, kind: Option<CardType>, modify_pool: bool,
    ) -> Result<Vec<Card>, &'static str> {
        let mut pool = super::card_pools::cards(character, self.is_multiplayer())?;
        if modify_pool { self.add_dingy_rug_reward_pool(&mut pool, is_card_reward)?; }
        // r7 generate_uniform_noncombat_cards first instantiates every candidate
        // while applying instance-level filters, then instantiates each selected
        // card again. Those throw-away factories still consume global card ids.
        let mut available = Vec::new();
        for id in pool {
            if !card_info(id, false).is_some_and(|info| info.rarity == rarity
                && kind.is_none_or(|kind| info.card_type == kind)) {
                continue;
            }
            // Only r7's legacy generate_uniform_noncombat_cards (Hefty Tablet)
            // instantiates filtered candidates. CardRewardGenerationOptions uses
            // canonical card_metadata instead; Cheese/SeaGlass/GlassEye do not
            // allocate those throw-away identities.
            let iid = if probe_candidates { self.run_ctx_mut().new_card_id() } else { 0 };
            let Some(card) = Card::from_factory(id, false, iid) else {
                continue;
            };
            if matches!(card.rarity, CardRarity::Basic | CardRarity::Ancient) {
                continue;
            }
            available.push(id);
        }
        let mut cards = Vec::new();
        for _ in 0..count {
            if available.is_empty() {
                break;
            }
            let pick = {
                let run = self.run_ctx_mut();
                run.rngs.rewards.choice_index(available.len())
            };
            let id = available.remove(pick);
            let iid = self.run_ctx_mut().new_card_id();
            let card = Card::from_factory(id, false, iid).ok_or("card factory missing")?;
            cards.push(card);
        }
        Ok(cards)
    }

    fn make_uniform_card_reward(
        &mut self,
        count: usize,
        character: crate::character::CardPool,
        rarity: CardRarity,
        skip_completes: bool,
        add_injury_after: bool,
    ) -> Result<RewardItem, &'static str> {
        let mut cards: ArrayVec<Card, 4> = self.generate_uniform_cards(count.min(3), character, rarity, true, false)?
            .into_iter().collect();
        self.apply_card_reward_relic_hooks(&mut cards, false);
        Ok(RewardItem {
            kind: RewardItemKind::Card {
                cards,
                closed: false,
                // HeftyTablet uses CardSelectCmd.FromChooseACardScreen, not CardReward.
                reroll: None,
                progress: Default::default(),
            },
            consumed: false,
            skippable: true,
            skip_completes,
            add_injury_after,
            neows_bones_final: false,
        })
    }

    fn add_card_to_deck(&mut self, id: CardId) -> Result<(), &'static str> {
        let iid = self.run_ctx_mut().new_card_id();
        let card = Card::from_factory(id, false, iid).ok_or("card factory missing")?;
        self.add_card_instance_to_deck(card)
    }

    fn add_card_instance_to_deck(&mut self, mut card: Card) -> Result<(), &'static str> {
        self.modify_card_being_added(&mut card)?;
        // CardPileCmd.Add(Deck): insertion stamps the current floor even for
        // clones carrying an older card's acquisition provenance.
        card.floor_added_to_deck=Some(self.run_ctx().total_floor);
        let run = self.run_ctx_mut();
        // Frozen run_state.py::add_card_instance_to_deck does not request a
        // reference template. Only operations that actually read it warm the cache.
        run.deck.push(card);
        run.after_run_deck_entry(run.deck.len() - 1);
        Ok(())
    }

    fn offer_lead_paperweight_choice(&mut self, count: usize) -> Result<(), &'static str> {
        let mut cards = ArrayVec::<Card, 4>::new();
        let mut chosen = Vec::new();
        for _ in 0..count.min(3) {
            let mut rarity = self.roll_base_card_rarity();
            let candidates = loop {
                let candidates: Vec<CardId> = super::card_pools::cards(CardPool::Colorless, self.is_multiplayer())?
                    .into_iter()
                    .filter(|id| !chosen.contains(id))
                    .filter(|id| {
                        Card::from_factory(*id, false, 0).is_some_and(|card| card.rarity == rarity)
                    })
                    .collect();
                if !candidates.is_empty() || rarity == CardRarity::Rare {
                    break candidates;
                }
                rarity = match rarity {
                    CardRarity::Common => CardRarity::Uncommon,
                    _ => CardRarity::Rare,
                };
            };
            if candidates.is_empty() {
                break;
            }
            let id = {
                let run = self.run_ctx_mut();
                candidates[run.rngs.rewards.choice_index(candidates.len())]
            };
            chosen.push(id);

            // r7 _pick_reward_card always consumes the upgrade roll when roll_upgrade is true.
            let upgrade_roll = self.run_ctx_mut().rngs.rewards.next_float(1.0);
            let scaling = if self.options.ascension >= 7 {
                0.125
            } else {
                0.25
            };
            let upgrade_odds = if rarity == CardRarity::Rare {
                0.0
            } else {
                self.current_act as f64 * scaling
            };
            let iid = self.run_ctx_mut().new_card_id();
            cards.push(
                Card::from_factory(id, upgrade_roll <= upgrade_odds, iid)
                    .ok_or("colorless card factory missing")?,
            );
        }
        self.apply_card_reward_relic_hooks(&mut cards, false);
        self.offer_card_grid("relic:LEAD_PAPERWEIGHT", cards.into_iter().collect(),
            0, 1, true, "Choose a card");
        Ok(())
    }

    fn offer_neow_card_bundles(&mut self) -> Result<(), &'static str> {
        // C# ScrollBoxes.AfterObtained: common/uncommon choices from Owner.Character.CardPool.
        let pool = super::card_pools::cards(self.character.into(), self.is_multiplayer())?;
        let common: Vec<CardId> = pool.iter().copied()
            .filter(|id| card_info(*id, false).is_some_and(|info| info.rarity == CardRarity::Common))
            .collect();
        let uncommon: Vec<CardId> = pool.into_iter()
            .filter(|id| card_info(*id, false).is_some_and(|info| info.rarity == CardRarity::Uncommon))
            .collect();
        let mut used = Vec::<CardId>::new();
        let mut bundles = ArrayVec::<ArrayVec<Card, 3>, 2>::new();
        for _ in 0..2 {
            let mut bundle = ArrayVec::<Card, 3>::new();
            // ScrollBoxes.cs:73-78: Defect rolls before EACH bundle. A Claw
            // bundle skips all ordinary selections and does not mark IDs used.
            if self.character==Character::Defect
                && self.run_ctx_mut().rngs.rewards.next_int_exclusive(0,100)<1 {
                for _ in 0..3 {
                    let iid=self.run_ctx_mut().new_card_id();
                    bundle.push(Card::from_factory(CardId::Claw,false,iid).ok_or("Claw factory missing")?);
                }
                bundles.push(bundle);
                continue;
            }
            for _ in 0..2 {
                let candidates: Vec<CardId> = common
                    .iter()
                    .copied()
                    .filter(|id| !used.contains(id))
                    .collect();
                if candidates.is_empty() {
                    return Err("Scroll Boxes common pool exhausted");
                }
                let pick = self
                    .run_ctx_mut()
                    .rngs
                    .rewards
                    .choice_index(candidates.len());
                let id = candidates[pick];
                used.push(id);
                let iid = self.run_ctx_mut().new_card_id();
                bundle.push(Card::from_factory(id, false, iid).ok_or("card factory missing")?);
            }
            let candidates: Vec<CardId> = uncommon
                .iter()
                .copied()
                .filter(|id| !used.contains(id))
                .collect();
            if candidates.is_empty() {
                return Err("Scroll Boxes uncommon pool exhausted");
            }
            let pick = self
                .run_ctx_mut()
                .rngs
                .rewards
                .choice_index(candidates.len());
            let id = candidates[pick];
            used.push(id);
            let iid = self.run_ctx_mut().new_card_id();
            bundle.push(Card::from_factory(id, false, iid).ok_or("card factory missing")?);
            bundles.push(bundle);
        }
        self.reward = RewardState::default();
        self.reward.items.push(RewardItem {
            kind: RewardItemKind::CardBundles { bundles },
            consumed: false,
            skippable: false,
            skip_completes: true,
            add_injury_after: false,
            neows_bones_final: false,
        });
        self.reward.active = Some(0);
        self.phase = RunPhase::CardReward;
        Ok(())
    }

    fn roll_event_potion(&mut self) -> Option<PotionId> {
        let candidates = super::potion_pools::cards(self.character)?;
        if candidates.is_empty() {
            return None;
        }
        let index = self
            .run_ctx_mut()
            .rngs
            .rewards
            .choice_index(candidates.len());
        Some(candidates[index])
    }

    fn roll_event_potion_rarity(&mut self, rarity: PotionRarity) -> Option<PotionId> {
        let candidates: Vec<PotionId> = super::potion_pools::cards(self.character)?
            .into_iter()
            .filter(|id| POTION_INFO[id.index()].rarity == rarity)
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let index = self
            .run_ctx_mut()
            .rngs
            .rewards
            .choice_index(candidates.len());
        Some(candidates[index])
    }

    fn roll_potion(&mut self) -> Option<PotionId> {
        let Some(pool) = super::potion_pools::cards(self.character) else {
            self.unported = Some("unsupported potion reward pool");
            return None;
        };
        let roll = self.run_ctx_mut().rngs.rewards.next_float(1.0);
        let rarity = if roll <= 0.10 {
            PotionRarity::Rare
        } else if roll <= 0.35 {
            PotionRarity::Uncommon
        } else {
            PotionRarity::Common
        };
        let candidates: ArrayVec<PotionId, 64> = pool.into_iter()
            .filter(|id| POTION_INFO[id.index()].rarity == rarity)
            .collect();
        if candidates.is_empty() {
            self.unported = Some("empty potion reward pool");
            return None;
        }
        let i = self
            .run_ctx_mut()
            .rngs
            .rewards
            .choice_index(candidates.len());
        Some(candidates[i])
    }

    fn generate_combat_rewards(&mut self, room: RoomType) {
        self.reward = RewardState::default();
        // Combat has already ended. Reward-generation hooks can fail closed,
        // but cannot leave the run in COMBAT (RewardsSet.Offer:119-161).
        self.phase = RunPhase::CardReward;
        if room == RoomType::Boss && self.current_act + 1 >= self.acts.len() {
            self.after_rewards();
            return;
        }
        let (mut low, mut high) = match room {
            RoomType::Monster => (10, 20),
            RoomType::Elite => (35, 45),
            RoomType::Boss => (100, 100),
            _ => (0, 0),
        };
        let fake_merchant = !self.fake_merchant_loot.is_empty();
        if fake_merchant {
            // FakeMerchantEventEncounter overrides both gold bounds with 300;
            // GoldReward.Populate still consumes its single rewards-RNG draw.
            low = 300;
            high = 300;
        } else if self.options.ascension >= 3 {
            low = (low as f64 * 0.75) as i32;
            high = (high as f64 * 0.75) as i32;
        }

        // with_rewards_from_room rolls potion availability before populate() rolls gold.
        let force_potion=self.run_ctx().has_owned_relic(crate::combat::PLAYER,RelicId::WhiteBeastStatue);
        // PotionRewardOdds.Roll returns BEFORE RNG and odds adjustments when
        // WhiteBeastStatue.ShouldForcePotionReward applies.
        let potion_drop=if force_potion {true}else{
            let potion_roll = self.run_ctx_mut().rngs.rewards.next_float(1.0) as f32;
            let threshold = self.potion_reward_odds + if room == RoomType::Elite { 0.125 } else { 0.0 };
            let drop = potion_roll < threshold;
            if drop {self.potion_reward_odds-=0.10;}else{self.potion_reward_odds+=0.10;}
            drop
        };

        // C# GremlinMercNormal.CalculateGoldProportion / RewardsSet.WithRewardsFromRoom:
        // no GoldReward is constructed at zero proportion, so Populate consumes no RNG.
        let proportion = if room == RoomType::Monster {
            self.combat.as_ref().map_or(1.0, |c| c.gold_proportion())
        } else { 1.0 };
        if proportion > 0.0 {
            low = (low as f64 * proportion).round_ties_even() as i32;
            high = (high as f64 * proportion).round_ties_even() as i32;
            let gold = self.run_ctx_mut().rngs.rewards.next_int(low as i64, high as i64) as i32;
            self.run_ctx_mut().gain_run_gold(gold);
            self.reward.replay_gold.push(gold);
        }

        if potion_drop {
            let Some(potion) = self.roll_potion() else { return; };
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Potion(potion),
                consumed: false,
                skippable: true,
                skip_completes: true,
                add_injury_after: false,
                neows_bones_final: false,
            });
        }

        let card_item = self.reward.items.len();
        let Some(item) = self.make_nr_card_reward(Some(room)) else { return; };
        self.reward.items.push(item);
        if room == RoomType::Elite {
            // Frozen RewardsSet populates the relic after the card reward.
            let relic = self.pull_player_relic(None, false);
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Relic(relic),
                consumed: false,
                skippable: true,
                skip_completes: true,
                add_injury_after: false,
                neows_bones_final: false,
            });
        }
        // C# RewardsSet.GenerateWithoutOffering populates initial fixed Heist
        // GoldRewards too, before reward modifiers.
        // RewardsSet.WithRewardsFromRoom appends this player's extras, in room
        // emission order. Populate precedes reward modifiers and display sorting.
        if let Some(card)=self.lantern_key_reward.take() {
            // RewardsSet.WithRewardsFromRoom appends CombatRoom.ExtraRewards
            // AFTER the ordinary room rewards; it never replaces their RNG,
            // gold, potion/card rewards or subsequent modifiers.
            self.reward.items.push(RewardItem {
                kind:RewardItemKind::SpecialCard {owner:crate::combat::PLAYER,card},
                consumed:false,skippable:false,skip_completes:true,
                add_injury_after:false,neows_bones_final:false,
            });
        }
        if std::mem::take(&mut self.punch_off_combat) {
            self.append_punch_off_rewards();
            if self.unported.is_some() { return; }
        }
        for id in std::mem::take(&mut self.fake_merchant_loot) {
            // RewardsSet.WithRewardsFromRoom appends the event's already-bound
            // rug/stock rewards after the ordinary Monster reward set.
            self.reward.items.push(RewardItem { kind: RewardItemKind::Relic(id),
                consumed: false, skippable: true, skip_completes: true,
                add_injury_after: false, neows_bones_final: false });
        }
        self.append_combat_end_rewards();
        // WongosMysteryTicket.TryModifyRewards: three populated RelicRewards
        // after the fifth AfterCombatEnd, once only.
        let tickets: Vec<_> = self.run_ctx().relics.iter().enumerate()
            .filter_map(|(i,r)| (r.id == RelicId::WongosMysteryTicket && r.counter >= 5 && !r.flag1).then_some(i))
            .collect();
        for i in tickets {
            for _ in 0..3 {
                let id = self.pull_player_relic(None, false);
                self.reward.items.push(RewardItem { kind: RewardItemKind::Relic(id),
                    consumed: false, skippable: true, skip_completes: true,
                    add_injury_after: false, neows_bones_final: false });
            }
            self.run_ctx_mut().relics[i].flag1 = true;
            self.run_ctx_mut().relics[i].enabled = false;
        }
        self.run_ctx_mut().extra_gold_reward = 0;
        // AmethystAubergine.TryModifyRewards adds an already-populated fixed
        // GoldReward. RewardsSet.cs:109 does NOT Populate it again (no RNG).
        if !(room == RoomType::Boss && self.current_act + 1 >= self.acts.len()) {
            let count = self.run_ctx().owned_relics(crate::combat::PLAYER)
                .filter(|r| r.id == RelicId::AmethystAubergine && r.enabled).count();
            for _ in 0..count { treasure::gain_gold(self.run_ctx_mut(), 15); }
        }
        // LavaRock.cs:38-63: after the first act's boss, append two ordinary
        // RelicRewards, then persist HasTriggered and disable the relic.
        if room==RoomType::Boss && self.current_act==0 {
            let owners:Vec<_>=self.run_ctx().relics.iter().enumerate().filter_map(|(i,r)|
                (r.owner==crate::combat::PLAYER&&r.id==RelicId::LavaRock&&!r.flag1).then_some(i)).collect();
            for index in owners {
                for _ in 0..2 {
                    let relic=self.pull_player_relic(None,false);
                    self.reward.items.push(RewardItem{
                        kind:RewardItemKind::Relic(relic),consumed:false,skippable:true,
                        skip_completes:true,add_injury_after:false,neows_bones_final:false,
                    });
                }
                let relic=&mut self.run_ctx_mut().relics[index];
                relic.flag1=true;
                relic.enabled=false;
            }
        }
        if room == RoomType::Elite && self.run_ctx().owned_relics(crate::combat::PLAYER)
            .any(|r| r.id == RelicId::BlackStar && r.enabled)
        {
            // BlackStar.cs TryModifyRewards: one additional populated RelicReward
            // for Owner, only after Elite combat (never another seat's reward).
            let relic = self.pull_player_relic(None, false);
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Relic(relic), consumed: false, skippable: true,
                skip_completes: true, add_injury_after: false, neows_bones_final: false,
            });
        }
        if room == RoomType::Elite && self.run_ctx().owned_relics(crate::combat::PLAYER)
            .any(|r| r.id == RelicId::WhiteStar && r.enabled)
        {
            // C# WhiteStar.TryModifyRewards: additional boss-rarity CardReward.
            let mut chosen = ArrayVec::new();
            let mut cards = ArrayVec::new();
            for _ in 0..3 {
                let rarity = self.roll_card_rarity(RoomType::Boss);
                let Some(card) = self.generate_reward_card(rarity, &chosen) else { return; };
                chosen.push(card.id);
                cards.push(card);
            }
            self.apply_encounter_reward_relic_hooks(&mut cards, RoomType::Boss);
            self.reward.items.push(RewardItem {
                kind: RewardItemKind::Card {
                    cards, closed: false,
                    reroll: Some(CardRewardRerollOptions::Encounter(RoomType::Boss)),
                    progress: Default::default(),
                },
                consumed: false, skippable: true, skip_completes: false,
                add_injury_after: false, neows_bones_final: false,
            });
        }
        // LastingCandy.BeforeCombatRewardOffered is once per rewards set, after
        // Populate and modifiers, and only when a CardReward is present.
        if self.reward.items.iter().any(|r| matches!(r.kind, RewardItemKind::Card { .. })) {
            for relic in self.run_ctx_mut().owned_relics_mut(crate::combat::PLAYER) {
                if relic.enabled && relic.id == RelicId::LastingCandy { relic.counter += 1; }
            }
        }
        self.reward.active = Some(card_item);
        self.phase = RunPhase::CardReward;
        self.apply_prayer_wheel_rewards(room);
    }

    fn append_combat_end_rewards(&mut self) {
        let owner = crate::combat::PLAYER; // Current persistent run's local owner.
        let pending = self.combat.as_mut().map(|c| std::mem::take(&mut c.end_rewards))
            .unwrap_or_default();
        for reward in pending {
            let target = match &reward {
                crate::combat::EndReward::Gold { owner, .. }
                | crate::combat::EndReward::Card { owner, .. }
                | crate::combat::EndReward::SpecialCard { owner, .. }
                | crate::combat::EndReward::CardRemoval { owner } => *owner,
            };
            if target != owner {
                // C# CombatRoom.ExtraRewards is per player. Another seat's
                // rewards must remain available for that seat, never player zero.
                self.combat.as_mut().unwrap().end_rewards.push(reward);
                continue;
            }
            let kind = match reward {
                crate::combat::EndReward::Card { owner: _, room } => {
                    // CardReward.Populate / CardFactory.CreateForReward: each
                    // of three cards draws rarity, candidate and upgrade in order.
                    let mut cards = ArrayVec::new();
                    let mut chosen = ArrayVec::new();
                    for _ in 0..3 {
                        let rarity = self.roll_card_rarity(room);
                        let Some(card) = self.generate_reward_card(rarity, &chosen) else { return; };
                        chosen.push(card.id);
                        cards.push(card);
                    }
                    self.apply_encounter_reward_relic_hooks(&mut cards, room);
                    RewardItemKind::Card { cards, closed: false,
                        reroll: Some(CardRewardRerollOptions::Encounter(room)),
                        progress: Default::default() }
                }
                crate::combat::EndReward::Gold { amount, .. } => {
                    // RunManager's current persistent run owner is PLAYER.
                    // Foreign-seat gold is retained above, with no draw here.
                    let gold = self.run_ctx_mut().rngs.rewards.next_int(amount as i64, amount as i64);
                    treasure::gain_gold(self.run_ctx_mut(), gold as i32);
                    continue;
                }
                crate::combat::EndReward::SpecialCard { owner, card } =>
                    RewardItemKind::SpecialCard { owner, card },
                crate::combat::EndReward::CardRemoval { owner } =>
                    RewardItemKind::CardRemoval { owner },
            };
            self.reward.items.push(RewardItem {
                kind, consumed: false, skippable: true, skip_completes: true,
                add_injury_after: false, neows_bones_final: false,
            });
        }
    }

    fn local_d_g_card_modifier(relic: &Relic) -> bool {
        // These C# D-G modifiers explicitly guard player/card.Owner == Owner.
        // This run manager's deck and reward list belong only to seat zero.
        !matches!(relic.id, RelicId::FrozenEgg | RelicId::FresnelLens
            | RelicId::Glitter | RelicId::DingyRug | RelicId::Driftwood)
            || relic.owner == crate::combat::PLAYER
    }

    fn modify_card_being_added(&mut self, card: &mut Card) -> Result<(), &'static str> {
        // C# FrozenEgg/MoltenEgg/ToxicEgg.TryModifyCardBeingAddedToDeck and
        // TryModifyCardRewardOptionsLate share their upgrade predicate;
        // FresnelLens uses Nimble.CanEnchant. C# RunState.CloneCard creates a
        // new mutable card via ClonePreservingMutability + AddCard.
        let relics: Vec<_> = self.run_ctx().owned_relics(card.owner)
            .filter(|r| r.enabled).map(|r| r.id).collect();
        for relic in relics {
            self.modify_card_for_relic(card, relic)?;
        }
        Ok(())
    }

    fn modify_card_for_relic(&mut self, card: &mut Card, relic: RelicId) -> Result<(), &'static str> {
        Self::modify_inventory_card(self.run_ctx_mut(), card, relic)
    }

    /// Shared by solo and owner-isolated multiplayer inventory acquisition.
    pub(super) fn modify_inventory_card(run: &mut RunCtx, card: &mut Card, relic: RelicId) -> Result<(), &'static str> {
            // MoltenEgg has no pickup hook. Its deck/reward modifier must
            // still never let a teammate's egg upgrade the local run's cards.
            if relic == RelicId::MoltenEgg && !run.relics.iter()
                .any(|r| r.id == relic && r.enabled && r.owner == card.owner) {
                return Ok(());
            }
            let egg = matches!((relic, card.card_type),
                (RelicId::FrozenEgg, CardType::Power) |
                (RelicId::MoltenEgg, CardType::Attack) |
                (RelicId::ToxicEgg, CardType::Skill));
            if egg && !card.upgraded {
                let enchantments = card.enchantments.clone();
                let owner = card.owner;
                shop::upgrade_template(run, card);
                card.owner = owner;
                for (name, amount) in enchantments {
                    if !crate::card::enchant::apply_static_enchantment(card, name, amount) {
                        return Err("unsupported upgrade enchantment");
                    }
                }
            }
            if relic == RelicId::FresnelLens && Self::event_can_enchant(card, "Nimble") {
                if !crate::card::enchant::apply_static_enchantment(card, "Nimble", 2) {
                    return Err("unsupported Fresnel Lens enchantment");
                }
            }
        Ok(())
    }

    fn apply_encounter_reward_relic_hooks(&mut self, cards: &mut ArrayVec<Card, 4>, odds: RoomType) {
        // LastingCandy.TryModifyCardRewardOptions is early, before all Late
        // modifiers, independent of inventory order. Other-source rewards and
        // noncombat Encounter rewards (Dream Catcher) must not activate it.
        if matches!(self.run_ctx().room_type, RoomType::Monster | RoomType::Elite | RoomType::Boss)
            && self.run_ctx().owned_relics(crate::combat::PLAYER)
                .any(|r| r.enabled && r.id == RelicId::LastingCandy && r.counter > 0 && r.counter % 2 == 1)
        {
            let result = (|| {
                let pool = self.reward_card_pool(self.character.into())?;
                let powers: Vec<_> = pool.into_iter().filter(|id|
                    card_info(*id, false).is_some_and(|c| c.card_type == CardType::Power)).collect();
                let fresh: Vec<_> = powers.iter().copied().filter(|id|
                    cards.iter().all(|card| card.id != *id)).collect();
                let candidates = if fresh.is_empty() { &powers } else { &fresh };
                if !candidates.is_empty() {
                    let rarity = self.roll_other_card_rarity(odds);
                    // NoModifyHooks / NoCardPoolModifications: the retained pool
                    // is already resolved; do not run hooks a second time.
                    cards.push(self.generate_card_from_pool(candidates, rarity, &[], true)?);
                }
                Ok::<_, &'static str>(())
            })();
            if let Err(reason) = result { self.unported = Some(reason); return; }
        }
        self.apply_card_reward_relic_hooks(cards, true);
    }

    fn apply_card_reward_relic_hooks(&mut self, cards: &mut ArrayVec<Card, 4>, is_card_reward: bool) {
        self.apply_card_reward_relic_hooks_slice(cards.as_mut_slice(), is_card_reward);
    }

    fn apply_card_reward_relic_hooks_slice(&mut self, cards: &mut [Card], is_card_reward: bool) {
        let run=self.run_ctx_mut();
        let mut relics=std::mem::take(&mut run.relics);
        let mut next_id=run.next_card_id;
        let room=run.room_type;
        let mut niche=run.rngs.niche.clone();
        let result=super::reward_modifiers::apply(&mut relics,cards,crate::combat::PLAYER,Some(room),
            is_card_reward,&mut next_id,&mut niche,|card,next_id| {
                run.next_card_id=*next_id;
                let owner=card.owner;
                let enchantments=card.enchantments.clone();
                shop::upgrade_template(run,card);card.owner=owner;
                for (name,amount) in enchantments {
                    if !crate::card::enchant::apply_static_enchantment(card,name,amount) {
                        return Err("unsupported reward upgrade enchantment");
                    }
                }
                *next_id=run.next_card_id;Ok(())
            });
        run.next_card_id=next_id;run.relics=relics;run.rngs.niche=niche;
        if let Err(reason)=result {self.unported=Some(reason);}
    }

    fn pull_player_relic(&mut self, rarity: Option<usize>, treasure: bool) -> RelicId {
        let target = rarity.unwrap_or_else(|| {
            let roll = if treasure {
                self.run_ctx_mut().rngs.treasure_room.next_float(1.0)
            } else {
                self.run_ctx_mut().rngs.rewards.next_float(1.0)
            };
            if roll < 0.5 {
                0
            } else if roll < 0.83_f32 as f64 {
                1
            } else {
                2
            }
        });
        // Same GetAvailableDeque logic as 1605d1a's MpWorld helper: prune
        // every rarity BEFORE escalation. Solo Player uses refresh=false
        // and never MoveToFallback, so there is no multiplayer fallback queue.
        self.prune_player_relic_bag();
        for idx in target..=2 {
            if let Some(id) = self.relic_bags.player[0][idx].first().copied() {
                self.relic_bags.player[0][idx].remove(0);
                for bag in &mut self.relic_bags.shared {
                    bag.retain(|x| *x != id);
                }
                return id;
            }
        }
        RelicId::Circlet
    }

    fn pull_shared_treasure_relic(&mut self) -> RelicId {
        let roll = self.run_ctx_mut().rngs.treasure_room.next_float(1.0);
        let target = if roll < 0.5 {
            0
        } else if roll < 0.83 {
            1
        } else {
            2
        };
        for idx in target..=2 {
            if let Some(id) = self.relic_bags.shared[idx].first().copied() {
                self.relic_bags.shared[idx].remove(0);
                return id;
            }
        }
        RelicId::Circlet
    }

    fn reroll_card_reward(&mut self) -> Result<(), &'static str> {
        if !self.run_ctx().owned_relics(crate::combat::PLAYER).any(|r| r.id == RelicId::Driftwood && r.enabled) {
            return Err("reroll requires Driftwood");
        }
        let active = self.reward.active.ok_or("no active card reward")?;
        let options = match &mut self.reward.items.get_mut(active).ok_or("invalid reward")?.kind {
            RewardItemKind::Card { closed: false, reroll, progress, .. } => {
                let options=reroll.take().ok_or("reward cannot reroll")?;
                // CardReward.cs Reroll: marks _hasBeenRerolled before Populate.
                progress.rerolls_used += 1;
                options
            }
            _ => return Err("no active card reward"),
        };
        // C# CardReward.Reroll clears its options and Populate regenerates with
        // RerollOptions. Keep the reward active and do not generate gold/potions.
        let cards = match options {
            CardRewardRerollOptions::CrystalSphere(rarity) => {
                let mut rng = self.reward.sphere_rng.take().ok_or("sphere reward RNG missing")?;
                let result = self.make_crystal_sphere_card_reward(&mut rng, rarity);
                self.reward.sphere_rng = Some(rng);
                match result?.kind {
                    RewardItemKind::Card { cards, .. } => cards,
                    _ => unreachable!(),
                }
            }
            CardRewardRerollOptions::FuturePotions { rarity, kind } =>
                self.generate_future_potion_cards(rarity, kind)?,
            CardRewardRerollOptions::EmptyPool => {
                // C# Kaleidoscope retains an empty reroll pool. CardFactory
                // consumes the base rarity roll, then throws for no allowed rarity.
                self.roll_base_card_rarity();
                self.unported = Some("CardReward reroll: empty retained card pool");
                return Ok(());
            }
            CardRewardRerollOptions::Uniform { character, count, rarity } => {
                let mut cards: ArrayVec<Card, 4> = self
                    .generate_uniform_cards(count, character, rarity, false, true)?
                    .into_iter()
                    .collect();
                self.apply_card_reward_relic_hooks(&mut cards, true);
                cards
            }
            CardRewardRerollOptions::Encounter(room) => {
                let mut cards = ArrayVec::new();
                let mut chosen = ArrayVec::new();
                for _ in 0..3 {
                    // CardReward.cs:101-102 retains the original creation options.
                    // DreamCatcher.cs:22 uses Encounter/RegularEncounter as well.
                    let rarity = self.roll_card_rarity(room);
                    let card = self.generate_reward_card(rarity, &chosen)
                        .ok_or("card reward generation failed")?;
                    chosen.push(card.id);
                    cards.push(card);
                }
                self.apply_encounter_reward_relic_hooks(&mut cards, room);
                cards
            }
            CardRewardRerollOptions::Noncombat {
                character, count, rarity, roll_upgrade, modify_pool,
            } => {
                let item = self.make_noncombat_card_reward_with_pool_modifications(
                    count, character, rarity, roll_upgrade, false, false, modify_pool)?;
                match item.kind {
                    RewardItemKind::Card { cards, .. } => cards,
                    _ => unreachable!(),
                }
            }
        };
        if let RewardItemKind::Card { cards: old, .. } = &mut self.reward.items[active].kind {
            *old = cards;
        }
        Ok(())
    }

    fn pick_card(&mut self, index: usize) -> Result<(), &'static str> {
        let active = self.reward.active.ok_or("no active card reward")?;
        let card = match &self
            .reward
            .items
            .get(active)
            .ok_or("invalid active reward")?
            .kind
        {
            RewardItemKind::Card {
                cards,
                closed: false,
                ..
            } => cards
                .get(index)
                .cloned()
                .ok_or("invalid card reward index")?,
            _ => return Err("no active card reward"),
        };
        self.add_card_instance_to_deck(card)?;
        if let RewardItemKind::Card { progress, .. } = &mut self.reward.items[active].kind {
            // CardReward.cs OnSelect: successful Add -> chosenCardIds.Add.
            progress.cards_picked += 1;
        }
        let add_injury = self.reward.items[active].add_injury_after;
        self.reward.items[active].consumed = true;
        self.reward.active = None;
        if add_injury {
            self.add_card_to_deck(CardId::Injury)?;
        }
        self.advance_rewards();
        Ok(())
    }

    fn pick_card_bundle(&mut self, index: usize) -> Result<(), &'static str> {
        let active = self.reward.active.ok_or("no active card bundle reward")?;
        let bundle = match &self
            .reward
            .items
            .get(active)
            .ok_or("invalid active reward")?
            .kind
        {
            RewardItemKind::CardBundles { bundles } => bundles
                .get(index)
                .cloned()
                .ok_or("invalid card bundle index")?,
            _ => return Err("no active card bundle reward"),
        };
        for card in bundle {
            self.add_card_instance_to_deck(card)?;
        }
        self.reward.items[active].consumed = true;
        self.reward.active = None;
        self.advance_rewards();
        Ok(())
    }

    fn skip_card_reward(&mut self) -> Result<(), &'static str> {
        if self.awaiting_next_act {
            self.awaiting_next_act = false;
            return self.transition_next_act();
        }
        let active = self.reward.active.ok_or("no active card reward")?;
        let completes = self.reward.items[active].skip_completes;
        let add_injury = self.reward.items[active].add_injury_after;
        if completes {
            self.reward.items[active].consumed = true;
            self.reward.active = None;
            if add_injury {
                self.add_card_to_deck(CardId::Injury)?;
            }
            self.advance_rewards();
            return Ok(());
        }
        match &mut self
            .reward
            .items
            .get_mut(active)
            .ok_or("invalid active reward")?
            .kind
        {
            RewardItemKind::Card { closed, .. } => *closed = true,
            _ => return Err("no active card reward"),
        }
        // Frozen r7 closes a generic card modal and leaves the reward in the
        // container. It is not consumed and may be reopened later.
        self.reward.active = None;
        Ok(())
    }

    fn sacrifice_card_reward(&mut self) -> Result<(), &'static str> {
        let active = self.reward.active.ok_or("no active card reward")?;
        if !matches!(self.reward.items.get(active).map(|item| &item.kind),
            Some(RewardItemKind::Card { closed: false, reroll: Some(_), .. }))
        {
            return Err("no active card reward");
        }
        let relic_index = self.run_ctx().relics.iter().position(|relic|
            relic.owner == crate::combat::PLAYER && relic.id == RelicId::PaelsWing && relic.enabled)
            .ok_or("Paels Wing sacrifice unavailable")?;

        // Frozen r7 _do_card_reward_sacrifice calls CardReward.on_skipped,
        // then completes the reward before invoking PaelsWing.OnSacrifice.
        let add_injury = self.reward.items[active].add_injury_after;
        self.reward.items[active].consumed = true;
        self.reward.active = None;
        if add_injury {
            self.add_card_to_deck(CardId::Injury)?;
        }

        let obtain = {
            let relic = &mut self.run_ctx_mut().relics[relic_index];
            relic.counter += 1;
            relic.counter % 2 == 0
        };
        if !obtain {
            self.advance_rewards();
            return Ok(());
        }

        // PaelsWing.cs: pull the next owner relic from the Rewards RNG/front
        // of bag, then run the ordinary RelicCmd.Obtain pickup chain. Preserve
        // the remaining reward set across any nested pickup reward/selector.
        let id = self.pull_player_relic(None, false);
        let parent = std::mem::take(&mut self.reward);
        self.reward_parents.push((parent, false));
        self.obtain_event_relic(id)?;
        if self.unported.is_none() && self.pending_run_choice.is_none()
            && self.reward.items.is_empty() && self.phase != RunPhase::RunOver
        {
            self.after_rewards();
        }
        Ok(())
    }

    fn advance_rewards(&mut self) {
        if self.reward.active.is_some() {
            return;
        }
        // NRewardsScreen.cs:278-305 retains the other reward buttons;
        // NRewardButton.cs:215-220 requires another explicit selection.
        if self.reward.stay_in_container && self.reward.items.iter().any(|item| !item.consumed) {
            self.phase = RunPhase::CardReward;
            return;
        }
        // r7 sts2_env/run/run_manager.py::_advance_post_combat_rewards iterates
        // (CardReward, CardBundlesReward, PotionReward, RelicReward) in this order.
        // Reward selection resumes the next queued modal. A closed card screen
        // stays in the RewardsSet container until explicitly reopened (CardReward).
        let next = self
            .reward
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.consumed
                && !matches!(item.kind, RewardItemKind::Card { closed: true, .. }
                    | RewardItemKind::SpecialCard { .. } | RewardItemKind::CardRemoval { .. }))
            .min_by_key(|(index, item)| (match item.kind {
                RewardItemKind::Card { .. } => 0,
                RewardItemKind::CardBundles { .. } => 1,
                RewardItemKind::Potion(_) => 2,
                RewardItemKind::Relic(_) => 3,
                RewardItemKind::SpecialCard { .. } | RewardItemKind::CardRemoval { .. } => unreachable!(),
            }, *index))
            .map(|(index, _)| index);
        let Some(index) = next else {
            if self.reward.items.iter().any(|item| !item.consumed) {
                self.phase = RunPhase::CardReward;
            } else {
                self.after_rewards();
            }
            return;
        };
        if let RewardItemKind::Card { closed, .. } = &mut self.reward.items[index].kind {
            *closed = false;
        }
        self.reward.active = Some(index);
        self.phase = RunPhase::CardReward;
    }

    fn ordered_remaining_reward_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .reward
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.consumed).then_some(index))
            .collect();
        // R7 displays the container by rewards_set_index (POTION=2, RELIC=3,
        // CARD/CARD_BUNDLE=5). Preserve backing indices because reward.active
        // stores an index into self.reward.items.
        indices.sort_by_key(|&index| {
            let order = match &self.reward.items[index].kind {
                RewardItemKind::Potion(_) => 2,
                RewardItemKind::Relic(_) => 3,
                RewardItemKind::SpecialCard { .. } => 4,
                RewardItemKind::CardRemoval { .. } => 7,
                RewardItemKind::Card { .. } | RewardItemKind::CardBundles { .. } => 5,
            };
            (order, index)
        });
        indices
    }

    fn remaining_reward_index(&self, reward_index: usize) -> Option<usize> {
        self.ordered_remaining_reward_indices()
            .get(reward_index)
            .copied()
    }

    fn open_reward(&mut self, reward_index: usize) -> Result<(), &'static str> {
        let index = self
            .remaining_reward_index(reward_index)
            .ok_or("invalid reward index")?;
        match &mut self.reward.items[index].kind {
            RewardItemKind::Card { closed, .. } => *closed = false,
            RewardItemKind::CardRemoval { owner } => {
                let owner = *owner;
                let deck = if owner == crate::combat::PLAYER {
                    &self.run_ctx().deck
                } else {
                    self.combat.as_ref().ok_or("reward owner combat missing")?.player_deck(owner)
                };
                let options = Self::removable_card_indices(deck).into_iter().map(|i| RunChoiceCard {
                    instance_id: deck[i].instance_id, card_id: deck[i].id, source_pile: "deck",
                }).collect::<Vec<_>>();
                self.reward.active = None;
                if options.is_empty() { return Ok(()); }
                // C# RewardSynchronizer.cs:317: cancelable, manual confirmation.
                self.pending_run_choice = Some(RunCardChoice {
                    wire_source: Some("reward.card_removal.open"),
                    prompt: "Choose a card to remove", options, selected: Vec::new(),
                    min_choices: 1, max_choices: 1, allow_skip: true, single_select: false,
                    effect: RunChoiceEffect::RemoveReward { owner, reward_index: index },
                    resume: ChoiceResume::MapChoice, reward_cards: Vec::new(),
                });
                self.phase = RunPhase::CardReward;
                return Ok(());
            }
            _ => return Err("reward is not a card reward"),
        }
        self.reward.active = Some(index);
        self.phase = RunPhase::CardReward;
        Ok(())
    }

    fn pick_special_card(&mut self, reward_index: usize) -> Result<(), &'static str> {
        let index = self.remaining_reward_index(reward_index).ok_or("invalid reward index")?;
        let (owner, card) = match &self.reward.items[index].kind {
            RewardItemKind::SpecialCard { owner, card } => (*owner, card.clone()),
            _ => return Err("reward is not a special card"),
        };
        // C# SpecialCardReward.cs:77-80: Add to deck only on selection, using
        // the retained original mutable card, including its existing identity.
        if owner == crate::combat::PLAYER {
            self.add_card_instance_to_deck(card)?;
        } else {
            self.combat.as_mut().ok_or("reward owner combat missing")?.push_player_deck_card(owner,card);
        }
        self.reward.items[index].consumed = true;
        self.reward.active = None;
        self.advance_rewards();
        Ok(())
    }

    fn skip_reward_set(&mut self) -> Result<(), &'static str> {
        if self.reward.items.iter().any(|item| !item.consumed)
            && !self.reward.items.iter().any(|item| !item.consumed && item.skippable)
        {
            return Err("rewards cannot be skipped");
        }
        for item in &mut self.reward.items {
            item.consumed = true;
        }
        self.reward.active = None;
        self.after_rewards();
        Ok(())
    }

    fn skip_active_reward(&mut self, relic: bool) -> Result<(), &'static str> {
        let active = self.reward.active.ok_or("no active reward")?;
        let valid = matches!(
            (
                &self
                    .reward
                    .items
                    .get(active)
                    .ok_or("invalid active reward")?
                    .kind,
                relic
            ),
            (RewardItemKind::Relic(_), true) | (RewardItemKind::Potion(_), false)
        );
        if !valid {
            return Err("active reward type mismatch");
        }
        if !self.reward.items[active].skippable {
            return Err("reward is mandatory");
        }
        self.reward.items[active].consumed = true;
        self.reward.active = None;
        self.advance_rewards();
        Ok(())
    }

    fn pick_potion(&mut self, reward_index: Option<usize>) -> Result<(), &'static str> {
        let index = match reward_index {
            Some(index) => self
                .remaining_reward_index(index)
                .ok_or("invalid reward index")?,
            None => self.reward.active.ok_or("no potion reward")?,
        };
        let id = match self.reward.items.get(index).ok_or("invalid reward")?.kind {
            RewardItemKind::Potion(id) => id,
            _ => return Err("reward is not a potion reward"),
        };
        let run = self.run_ctx_mut();
        if let Some(slot) = run.potions.iter().position(Option::is_none) {
            run.potions[slot] = Some(id);
        } else if run.potions.len() < run.max_potion_slots as usize {
            run.potions.push(Some(id));
        } else {
            return Err("potion belt full");
        }
        self.reward.items[index].consumed = true;
        self.reward.active = None;
        self.advance_rewards();
        Ok(())
    }

    fn pick_relic_reward(&mut self, reward_index: Option<usize>) -> Result<(), &'static str> {
        let index = match reward_index {
            Some(index) => self
                .remaining_reward_index(index)
                .ok_or("invalid reward index")?,
            None => self.reward.active.ok_or("no relic reward")?,
        };
        let id = match self.reward.items.get(index).ok_or("invalid reward")?.kind {
            RewardItemKind::Relic(id) => id,
            _ => return Err("reward is not a relic reward"),
        };
        // The marker identifies a Bones RewardsSet, NOT a particular relic's
        // selection order. C# waits for the entire mandatory set and its pickups.
        let bones_set = self.reward.items.iter().any(|item| item.neows_bones_final);
        let is_wax = self.reward.wax_relics;
        self.reward.items[index].consumed = true;
        self.reward.active = None;
        let final_curse = bones_set && self.reward.items.iter().all(|item| item.consumed);
        let parent = std::mem::take(&mut self.reward);
        self.reward_parents.push((parent, final_curse));
        let obtained=self.run_ctx().relics.len();
        self.obtain_event_relic(id)?;
        if is_wax {
            if let Some(relic) = self.run_ctx_mut().relics.get_mut(obtained) {
                relic.is_wax = true;
            }
        }
        if self.unported.is_none() && self.pending_run_choice.is_none()
            && self.reward.items.is_empty() && self.phase != RunPhase::RunOver
        {
            self.after_rewards();
        }
        Ok(())
    }

    fn after_rewards(&mut self) {
        if let Some((parent, final_curse)) = self.reward_parents.pop() {
            if final_curse {
                if let Err(reason) = self.add_neows_bones_curse() {
                    self.unported = Some(reason);
                    return;
                }
            }
            self.reward = parent;
            self.advance_rewards();
            return;
        }
        if self.native_event_id() == Some(EventId::RanwidTheElder) {
            if let Err(reason) = self.resume_ranwid_pickups() { self.unported = Some(reason); }
            return;
        }
        if self.native_event_id() == Some(EventId::WelcomeToWongos) {
            if let Err(reason) = self.finish_wongo_purchase() { self.unported = Some(reason); }
            return;
        }
        let room = self.run_ctx().room_type;
        if self.current_event.as_ref().is_some_and(|e| e.id == EventId::EndlessConveyor) {
            self.resume_conveyor();
            self.phase = RunPhase::Event;
            return;
        }
        if room == RoomType::Shop {
            self.phase = RunPhase::Shop;
            return;
        }
        if room == RoomType::RestSite && self.reward_returns_to_rest {
            self.reward_returns_to_rest = false;
            self.phase = RunPhase::RestSite;
            return;
        }
        self.reward_returns_to_rest = false;
        if room == RoomType::Boss {
            if self.current_act + 1 < self.acts.len() {
                self.awaiting_next_act = true;
                self.phase = RunPhase::CardReward;
            } else {
                let map = self.acts[self.current_act].map.as_ref().unwrap();
                // NRewardsScreen.OnProceedButtonPressed: the second boss is
                // reached only from the first boss's actual map coordinate.
                // A coordinate-free `room boss` must enter TheArchitect.
                if map.second_boss.is_some() && self.run_ctx().coord.is_some()
                    && self.current_point == map.boss {
                    self.phase = RunPhase::MapChoice;
                } else {
                    self.phase = RunPhase::Event;
                    // TheArchitect.cs:201-303: initial (zero-win profile)
                    // dialogue lengths. LoadDialogue:310-315 draws even for
                    // the single eligible dialogue; no run RNG is consumed.
                    let mut event = EventState::new(EventId::TheArchitect,
                        Rng::new(self.master_seed.wrapping_add(crate::rng::deterministic_hash64("THE_ARCHITECT"))));
                    event.rng.choice_index(1);
                    event.ints[0] = match self.character {
                        Character::Silent => 1,
                        Character::Ironclad | Character::Necrobinder => 2,
                        _ => 3,
                    };
                    event.options.push(EventOption::new(
                        if event.ints[0] > 1 { "advance" } else { "proceed" }, "Continue", ""));
                    self.current_event = Some(event);
                    self.unported = None;
                }
            }
        } else {
            self.phase = RunPhase::MapChoice;
        }
    }

    fn transition_next_act(&mut self) -> Result<(), &'static str> {
        if self.current_act + 1 >= self.acts.len() {
            self.phase = RunPhase::RunOver;
            return Ok(());
        }
        self.set_act_internal(self.current_act+1)
    }

    fn set_act_internal(&mut self,act:usize)->Result<(),&'static str> {
        if act>=self.acts.len() {return Err("debug act does not exist");}
        self.current_act=act;
        self.act_floor = 0;
        let def = self.acts[self.current_act].def;
        let mut map = generate_act_map(
            def.num_rooms,
            &mut self.map_rngs[self.current_act],
            self.options.ascension,
            self.current_act,
        );
        // C# SpoilsMap.AfterCreated / ModifyGeneratedMap: only Act 2 and only
        // while this quest card remains in the deck. The ordinary map was
        // generated first, so its act_N_map stream still advances normally.
        if self.current_act == 1
            && self.run_ctx().deck.iter().any(|card| card.id == CardId::SpoilsMap)
        {
            map = generate_spoils_act_map(def.num_rooms, self.run_ctx().rngs.seed,
                self.options.ascension, self.current_act);
            // C# SpoilsMap.ModifyGeneratedMapLate / AfterMapGenerated: remember
            // the treasure coordinate by attaching each held quest instance.
            if let Some(treasure) = map.points.iter_mut()
                .find(|point| point.point_type == MapPointType::Treasure)
            {
                treasure.spoils_quests.extend(self.run_ctx().deck.iter()
                    .filter(|card| card.id == CardId::SpoilsMap)
                    .map(|card| card.instance_id));
            }
        }
        let start = map.start;
        self.acts[self.current_act].map = Some(map);
        self.current_point = start;
        self.visited_points.clear();
        self.unknown_odds = UNKNOWN_BASE;
        let current_act = self.current_act as i32;
        let run = self.run_ctx_mut();
        run.act_index = current_act;
        run.coord = None;
        self.phase = RunPhase::MapChoice;
        Ok(())
    }

    /// Fresh-run subset of Core/DevConsole/ConsoleCommands/ActConsoleCmd.cs
    /// (integer argument -> RunManager.EnterAct -> SetActInternal, no inventory
    /// or HP grants), followed by RoomConsoleCmd.cs "room boss".
    /// C# Runs/RunManager.cs:881-946 appends history without visiting a map
    /// coordinate; RunState.cs:339-355 therefore increments TotalFloor only.
    pub fn new_debug_late_boss(seed:u64,options:RunOptions,character:super::mp::Character,act:usize)
        ->Result<Self,&'static str> {
        let mut mgr=Self::new_debug_act(seed,options,character,act)?;
        mgr.map_history.push(MapHistoryEntry{map_point_type:MapPointType::Boss,room_type:RoomType::Boss});
        mgr.run_ctx_mut().total_floor+=1;
        let encounter=mgr.acts[act].rooms.boss;
        rest::after_room_entered(mgr.run_ctx_mut(),RoomType::Boss,MapPointType::Boss);
        mgr.enter_encounter(RoomType::Boss,encounter)?;
        Ok(mgr)
    }

    /// ActConsoleCmd.cs integer argument -> EnterAct -> SetActInternal.
    /// Starts on MapRoom with no visited coordinate; the starting Ancient
    /// remains the first legal destination, not an implicitly visited node.
    pub fn new_debug_act(seed:u64,options:RunOptions,character:super::mp::Character,act:usize)
        ->Result<Self,&'static str> {
        if !(1..=2).contains(&act) {return Err("debug coverage requires Act 2 or 3");}
        let mut mgr=Self::new_for_character(seed,options,character);
        mgr.set_act_internal(act)?;mgr.unported=None;
        Ok(mgr)
    }

    /// WinConsoleCmd.cs kills a snapshot of enemies, strips their powers and
    /// checks victory, then uses the ordinary run carryback/reward continuation.
    pub fn debug_win_combat(&mut self)->Result<(),&'static str> {
        if self.phase!=RunPhase::Combat {return Err("debug win requires combat");}
        let combat=self.combat.as_mut().ok_or("combat missing")?;
        if !combat.is_over {combat.debug_win_combat()?;}
        self.after_combat_action()
    }

    fn enter_treasure(&mut self) -> Result<(), &'static str> {
        self.treasure_relic = if treasure::should_generate(self.run_ctx()) {
            Some(self.pull_shared_treasure_relic())
        } else {
            None
        };
        self.phase = RunPhase::Treasure;
        self.unported = None;
        Ok(())
    }

    fn collect_treasure(&mut self) -> Result<(), &'static str> {
        let Some(id) = self.treasure_relic.take() else {
            self.phase = RunPhase::MapChoice;
            return Ok(());
        };
        // OneOffSynchronizer.DoTreasureRoomRewards pays gold on chest open,
        // before the reward relic is selected and obtained.
        let _ = treasure::chest_gold(self.run_ctx_mut(), id);
        self.obtain_event_relic(id)?;
        if self.unported.is_some() {
            return Ok(());
        }
        // C# OneOffSynchronizer.DoTreasureRoomRewards / TryHandleSpoilsMap:
        // chest gold precedes the quest payout, and a marked current point is
        // required (a quest added after map generation does not mark it).
        let spoils: Vec<u32> = self.run_ctx().deck.iter()
            .filter(|card| card.id == CardId::SpoilsMap)
            .map(|card| card.instance_id).collect();
        let marked = self.acts[self.current_act].map.as_ref().unwrap()
            .points[self.current_point as usize].spoils_quests.iter()
            .any(|instance| spoils.contains(instance));
        if marked {
            // SpoilsMap.OnQuestComplete: gain modified gold, then remove this
            // exact card. BeforeCardRemoved clears its map quest registration.
            for instance in spoils {
                treasure::gain_gold(self.run_ctx_mut(), 600);
                self.run_ctx_mut().retain_run_deck_cards(|card| card.instance_id != instance);
            }
            self.acts[self.current_act].map.as_mut().unwrap()
                .points[self.current_point as usize].spoils_quests.clear();
        }
        self.phase = RunPhase::MapChoice;
        Ok(())
    }
}

/// Whether `key` names a selector or event-option table shape admitted by evidence recorded
/// from the original game. The list holds SHA-256 digests of those keys (hex, one per line);
/// the keys themselves contain the game's option text and are not published.
pub fn recorded_selector_key(key:&str) -> bool {
    use sha2::{Digest,Sha256};
    let digest:String=Sha256::digest(key.as_bytes()).iter().map(|b|format!("{b:02x}")).collect();
    include_str!("selector_shapes.sha256").lines().any(|line|line==digest)
}