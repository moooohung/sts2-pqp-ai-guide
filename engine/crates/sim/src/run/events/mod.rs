//! Frozen-r7 run-event state shared by the EVENT packet.
//!
//! Behavioural dispatch stays explicit in `run::manager`: this module owns the compact
//! event/choice state so normal run stepping never needs Python-shaped dynamic objects.

use arrayvec::ArrayVec;
pub(crate) mod doll_room;
pub(crate) mod spirit_grafter;
pub(crate) mod lantern_key;
pub(crate) mod infested_automaton;
use std::sync::OnceLock;

use crate::rng::Rng;
use crate::tables::{CardId, CardRarity, PotionRarity, RelicId};

pub(crate) mod native_identity;
pub(crate) mod native_offer;
#[cfg(test)]
mod native_identity_tests;
pub(crate) use native_identity::{EventOptionIdentity, EventOptions, EventParameter, NativeEventOption};
use crate::identity::{EventId, EventPageId};

include!(concat!(env!("OUT_DIR"), "/gen_event_identity.rs"));
pub(crate) mod tinker_time;
pub(crate) mod trial;

pub(crate) const MAX_EVENT_OPTIONS: usize = 16;
pub(crate) fn sphere_reward_option_id(index:usize,action:usize)->&'static str {
    // PopulateItems makes 15 items per attempt, with at most ten attempts.
    static IDS:OnceLock<Vec<String>>=OnceLock::new();
    &IDS.get_or_init(||(0..150).flat_map(|i|
        (0..7).map(move |a|format!("sphere_reward_{i}_{a}"))).collect())[index*7+action]
}
pub(crate) const MAX_EVENT_CARDS: usize = 24;
pub(crate) const MAX_EVENT_RELICS: usize = 8;
pub(crate) const MAX_EVENT_EFFECTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventDescription {
    DenseVegetationTrudgeGold(i32),
    DenseVegetationRestHeal(i32),
    ColossalFlowerExtractGold(i32),
    ColossalFlowerReachDeeperDamage(i32),
    ColossalFlowerPollinousCoreDamage(i32),
    SunkenTreasurySmallGold(i32),
    SunkenTreasuryLargeGold(i32),
    SunkenStatueDive(i32),
    LostWispSearchGold(i32),
    UnrestSiteHeal(i32),
    SpiralingWhirlpoolHeal(i32),
    ThisOrThatPlainGold(i32),
    TabletDecipherCost(i32),
    JungleSoloGold(u64),
    JungleJoinGold(u64),
    BridgeHoldOn(i32),
    FakeMerchantCost(i32),
}

impl EventDescription {
    pub(crate) fn render(self) -> String {
        match self {
            Self::DenseVegetationTrudgeGold(gold) => {
                format!("Take 8 damage, gain {gold} gold")
            }
            Self::DenseVegetationRestHeal(heal) => format!("Heal {heal} HP, then fight"),
            Self::ColossalFlowerExtractGold(gold) => format!("Gain {gold} gold"),
            Self::ColossalFlowerReachDeeperDamage(damage) => {
                format!("Take {damage} damage for better reward")
            }
            Self::ColossalFlowerPollinousCoreDamage(damage) => {
                format!("Take {damage} damage, gain Pollinous Core relic")
            }
            Self::SunkenTreasurySmallGold(gold) => format!("Gain {gold} gold"),
            Self::SunkenTreasuryLargeGold(gold) => {
                format!("Gain {gold} gold + Greed curse")
            }
            Self::SunkenStatueDive(gold) => format!("Gain {gold} gold, take 7 damage"),
            Self::LostWispSearchGold(gold) => format!("Gain {gold} gold"),
            Self::UnrestSiteHeal(heal) => format!("Heal {heal} HP, gain Poor Sleep curse"),
            Self::SpiralingWhirlpoolHeal(heal) => format!("Heal {heal} HP"),
            Self::ThisOrThatPlainGold(gold) => format!("Take 6 damage, gain {gold} gold"),
            Self::TabletDecipherCost(cost) => format!("Lose {cost} Max HP, upgrade a card"),
            Self::JungleSoloGold(bits) => format!("Take 18 damage, gain {:?} gold", f64::from_bits(bits)),
            Self::JungleJoinGold(bits) => format!("Gain {:?} gold", f64::from_bits(bits)),
            Self::BridgeHoldOn(damage) => format!("Take {damage} damage, reroll card"),
            Self::FakeMerchantCost(gold) => format!("Pay {gold} gold for this relic"),
        }
    }
}

/// Menu-authoring/compatibility input. EventState stores NativeEventOption instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventOption {
    pub option_id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub dynamic_description: Option<EventDescription>,
    pub enabled: bool,
}

impl EventOption {
    pub const fn new(
        option_id: &'static str,
        label: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            option_id,
            label,
            description,
            dynamic_description: None,
            enabled: true,
        }
    }

    pub const fn with_dynamic_description(mut self, description: EventDescription) -> Self {
        self.dynamic_description = Some(description);
        self
    }

    pub const fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BridgeState {
    pub current: Option<u32>,
    pub skipped: Vec<u32>,
    pub hold_ons: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SphereItemKind {
    Relic,
    Potion(PotionRarity),
    Card(CardRarity),
    Curse,
    Gold(i32),
}

#[derive(Clone, Debug)]
struct SphereItem {
    kind: SphereItemKind,
    width: u8,
    height: u8,
    position: Option<(u8, u8)>,
    reveal_subscriptions: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct SphereState {
    pub picks_remaining: i32,
    hidden: [bool; 121],
    cells: [i16; 121],
    items: Vec<SphereItem>,
    revealed: Vec<usize>,
}

impl SphereState {
    #[inline]
    fn cell(x: usize, y: usize) -> usize {
        x * 11 + y
    }

    pub(crate) fn new(rng: &mut Rng, picks: i32) -> Self {
        let mut out = Self {
            picks_remaining: picks,
            hidden: [true; 121],
            cells: [-1; 121],
            items: Vec::new(),
            revealed: Vec::new(),
        };

        // Frozen r7 events/crystal_sphere.py expands the four corners twice,
        // then permanently reveals that public border region.
        let mut corners = vec![(0usize, 0usize), (10, 0), (10, 10), (0, 10)];
        for _ in 0..2 {
            let prior = corners.clone();
            let mut next = prior.clone();
            for &(x, y) in &prior {
                if x > 0 {
                    next.push((x - 1, y));
                }
                if x < 10 {
                    next.push((x + 1, y));
                }
            }
            for &(x, y) in &prior {
                if y > 0 {
                    next.push((x, y - 1));
                }
                if y < 10 {
                    next.push((x, y + 1));
                }
            }
            corners = next;
        }
        for (x, y) in corners {
            out.hidden[Self::cell(x, y)] = false;
        }

        for _ in 0..10 {
            if out.populate_items(rng) {
                break;
            }
        }
        out
    }

    fn populate_items(&mut self, rng: &mut Rng) -> bool {
        let specs = [
            (SphereItemKind::Relic, 4, 4),
            (SphereItemKind::Potion(PotionRarity::Common), 1, 3),
            (SphereItemKind::Potion(PotionRarity::Common), 1, 3),
            (SphereItemKind::Potion(PotionRarity::Rare), 2, 2),
            (SphereItemKind::Card(CardRarity::Common), 2, 2),
            (SphereItemKind::Card(CardRarity::Uncommon), 2, 2),
            (SphereItemKind::Card(CardRarity::Rare), 2, 2),
            (SphereItemKind::Curse, 2, 2),
            (SphereItemKind::Gold(10), 1, 1),
            (SphereItemKind::Gold(10), 1, 1),
            (SphereItemKind::Gold(10), 1, 1),
            (SphereItemKind::Gold(10), 1, 1),
            (SphereItemKind::Gold(10), 1, 1),
            (SphereItemKind::Gold(30), 2, 1),
            (SphereItemKind::Gold(30), 2, 1),
        ];
        let mut success = true;
        for (kind, width, height) in specs {
            let item_index = self.items.len();
            let mut item = SphereItem {
                kind,
                width,
                height,
                position: None,
                reveal_subscriptions: 0,
            };
            if success {
                let mut positions = Vec::new();
                for x in 0..=11 - width as usize {
                    for y in 0..=11 - height as usize {
                        let fits = (0..width as usize).all(|dx| {
                            (0..height as usize).all(|dy| {
                                let cell = Self::cell(x + dx, y + dy);
                                self.hidden[cell] && self.cells[cell] < 0
                            })
                        });
                        if fits {
                            positions.push((x, y));
                        }
                    }
                }
                if positions.is_empty() {
                    success = false;
                } else {
                    let (x, y) = positions[rng.choice_index(positions.len())];
                    item.position = Some((x as u8, y as u8));
                    for dx in 0..width as usize {
                        for dy in 0..height as usize {
                            self.cells[Self::cell(x + dx, y + dy)] = item_index as i16;
                        }
                    }
                }
            }
            self.items.push(item);
        }
        // PopulateItems re-subscribes every item after each failed attempt.
        for item in &mut self.items {
            item.reveal_subscriptions = item.reveal_subscriptions.saturating_add(1);
        }
        success
    }

    pub(crate) fn options(&self) -> Vec<NativeEventOption> {
        let mut out = Vec::new();
        for x in 0..11 {
            for y in 0..11 {
                if !self.hidden[Self::cell(x, y)] {
                    continue;
                }
                for big in [true, false] {
                    out.push(NativeEventOption::sphere(big, x as u8, y as u8));
                }
            }
        }
        out
    }

    pub(crate) fn hidden_cells(&self) -> Vec<(usize, usize)> {
        (0..11).flat_map(|x| (0..11).filter_map(move |y|
            self.hidden[Self::cell(x, y)].then_some((x, y)))).collect()
    }

    pub(crate) fn click(&mut self, option_id: EventOptionIdentity) -> Result<usize, &'static str> {
        if self.picks_remaining <= 0 {
            return Err("crystal sphere is finished");
        }
        let EventParameter::Sphere { big, x, y } = option_id.parameter else {
            return Err("invalid crystal sphere option");
        };
        if !option_id.is(crate::identity::EventOptionId::CrystalSphereCrystalSphereMinigameCellClicked)
            || x >= 11
            || y >= 11
            || !self.hidden[Self::cell(x as usize, y as usize)]
        {
            return Err("invalid crystal sphere option");
        }
        let (x, y) = (x as usize, y as usize);

        self.picks_remaining -= 1;
        let mut cells = Vec::new();
        if !big {
            cells.push((x, y));
        } else {
            // C# GetAdjacentCells: horizontal, vertical, diagonal, center.
            // Reveal order is observable through reward ordering and RNG.
            for dx in [-1isize, 1] {
                let xx = x as isize + dx;
                if (0..11).contains(&xx) {
                    cells.push((xx as usize, y));
                }
            }
            for dy in [-1isize, 1] {
                let yy = y as isize + dy;
                if (0..11).contains(&yy) {
                    cells.push((x, yy as usize));
                }
            }
            for dx in [-1isize, 1] {
                for dy in [-1isize, 1] {
                    let xx = x as isize + dx;
                    let yy = y as isize + dy;
                    if (0..11).contains(&xx) && (0..11).contains(&yy) {
                        cells.push((xx as usize, yy as usize));
                    }
                }
            }
            cells.push((x, y));
        }

        let mut curses = 0usize;
        for (cx, cy) in cells {
            let cell = Self::cell(cx, cy);
            if !self.hidden[cell] {
                continue;
            }
            self.hidden[cell] = false;
            let item_index = self.cells[cell];
            if item_index < 0 {
                continue;
            }
            let item_index = item_index as usize;
            let item = &self.items[item_index];
            let Some((px, py)) = item.position else {
                continue;
            };
            let all_revealed = (0..item.width as usize).all(|dx| {
                (0..item.height as usize).all(|dy| {
                    !self.hidden[Self::cell(px as usize + dx, py as usize + dy)]
                })
            });
            if all_revealed {
                self.revealed.extend(std::iter::repeat_n(
                    item_index,
                    item.reveal_subscriptions as usize,
                ));
                if item.kind == SphereItemKind::Curse {
                    curses += 1;
                }
            }
        }
        Ok(curses)
    }

    pub(crate) fn revealed_rewards(&self) -> Vec<SphereItemKind> {
        self.revealed
            .iter()
            .filter_map(|&index| self.items.get(index).map(|item| item.kind))
            .filter(|kind| *kind != SphereItemKind::Curse)
            .collect()
    }
}

fn sphere_option_id(big: bool, x: usize, y: usize) -> &'static str {
    static IDS: OnceLock<Vec<String>> = OnceLock::new();
    let ids = IDS.get_or_init(|| {
        (0..11)
            .flat_map(|x| {
                (0..11).flat_map(move |y| {
                    [format!("sphere_big_{x}_{y}"), format!("sphere_small_{x}_{y}")]
                })
            })
            .collect()
    });
    ids[(x * 11 + y) * 2 + usize::from(!big)].as_str()
}

fn sphere_option_label(big: bool, x: usize, y: usize) -> &'static str {
    static LABELS: OnceLock<Vec<String>> = OnceLock::new();
    let labels = LABELS.get_or_init(|| {
        (0..11)
            .flat_map(|x| {
                (0..11).flat_map(move |y| {
                    [format!("Big ({x}, {y})"), format!("Small ({x}, {y})")]
                })
            })
            .collect()
    });
    labels[(x * 11 + y) * 2 + usize::from(!big)].as_str()
}

#[derive(Clone, Debug)]
pub(crate) struct EventState {
    pub id: crate::identity::EventId,
    pub rng: Rng,
    pub options: EventOptions,
    /// Compact event-local scratch matching r7 instance attributes.
    pub ints: [i32; 8],
    pub flags: u16,
    pub cards: ArrayVec<CardId, MAX_EVENT_CARDS>,
    pub relics: ArrayVec<RelicId, MAX_EVENT_RELICS>,
    pub bridge: BridgeState,
    pub crystal: Option<SphereState>,
}

impl EventState {
    pub fn new(id: crate::identity::EventId, rng: Rng) -> Self {
        Self {
            id,
            rng,
            options: EventOptions::new(id),
            ints: [0; 8],
            flags: 0,
            cards: ArrayVec::new(),
            relics: ArrayVec::new(),
            bridge: BridgeState::default(),
            crystal: None,
        }
    }

    /// AbyssalBaths.Linger selects its warning page after damage has resolved.
    pub(crate) fn refresh_public_page(&mut self, hp: i32) {
        if self.id == crate::identity::EventId::AbyssalBaths && self.ints[1] > 0 {
            let page = if hp <= self.ints[0] - 2 { EventPageId::AbyssalBathsDeathWarning }
                else { EventPageId::AbyssalBathsLingerLingerCount };
            self.options.set_page(page, if page == EventPageId::AbyssalBathsDeathWarning {
                EventParameter::None
            } else { EventParameter::Counter(self.ints[1]) });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventEffect {
    LoseHp(i32),
    Heal(i32),
    GainMaxHp(i32),
    LoseMaxHp(i32),
    GainGold(i32),
    LoseGold(i32),
    AddCard(CardId),
    AddRelic(RelicId),
    UpgradeRandomDeckCards(usize),
    RollTrashRelic,
}

#[derive(Clone, Debug)]
pub(crate) struct EventResolution {
    pub finished: bool,
    pub effects: ArrayVec<EventEffect, MAX_EVENT_EFFECTS>,
    pub next_options: ArrayVec<EventOption, MAX_EVENT_OPTIONS>,
}

impl EventResolution {
    fn finished() -> Self {
        Self {
            finished: true,
            effects: ArrayVec::new(),
            next_options: ArrayVec::new(),
        }
    }

    fn continuing() -> Self {
        Self {
            finished: false,
            effects: ArrayVec::new(),
            next_options: ArrayVec::new(),
        }
    }
}

#[inline]
fn abyssal_damage_description(damage: i32) -> &'static str {
    match damage {
        3 => "+2 Max HP, take 3 damage",
        4 => "+2 Max HP, take 4 damage",
        5 => "+2 Max HP, take 5 damage",
        6 => "+2 Max HP, take 6 damage",
        7 => "+2 Max HP, take 7 damage",
        8 => "+2 Max HP, take 8 damage",
        9 => "+2 Max HP, take 9 damage",
        10 => "+2 Max HP, take 10 damage",
        11 => "+2 Max HP, take 11 damage",
        12 => "+2 Max HP, take 12 damage",
        _ => "+2 Max HP, take 13 damage",
    }
}

/// Exact fail-closed boundary for catalog events whose r7 behavior still needs
/// run-layer features this packet has not represented.  Keep the source class
/// location in the reason so corpus reports identify the concrete remaining gate.
pub(crate) fn unported_reason(id: crate::identity::EventId) -> &'static str {
    match id {
        EventId::BrainLeech => "EVENT:BRAIN_LEECH r7 act1.py:37-90 CardReward flow",
        EventId::JungleMazeAdventure => {
            "EVENT:JUNGLE_MAZE_ADVENTURE r7 act2.py:638-697 event RNG variable flow"
        }
        EventId::LuminousChoir => {
            "EVENT:LUMINOUS_CHOIR r7 act2.py:705-773 random relic/remove reward flow"
        }
        EventId::SapphireSeed => {
            "EVENT:SAPPHIRE_SEED r7 shared.py:1552-1602 upgrade/enchant choice reward"
        }
        EventId::SelfHelpBook => "EVENT:SELF_HELP_BOOK r7 shared.py:1610-1681 typed enchant choice",
        EventId::StoneOfAllTime => {
            "EVENT:STONE_OF_ALL_TIME r7 act2.py:1169-1245 post-choice RNG/enchant flow"
        }
        EventId::TabletOfTruth => "EVENT:TABLET_OF_TRUTH r7 shared.py:1832-1911 run-loss/upgrade flow",
        EventId::TheFutureOfPotions => {
            "EVENT:THE_FUTURE_OF_POTIONS r7 act2.py:1326-1407 potion/card-type reward"
        }
        EventId::WelcomeToWongos => "EVENT:WELCOME_TO_WONGOS missing explicit SaveManager.Progress.WongoPoints input",
        EventId::WhisperingHollow => {
            "EVENT:WHISPERING_HOLLOW r7 act2.py:1688-1764 event-RNG transform/potion rewards"
        }
        EventId::Darv => "EVENT:DARV r7 shared.py:903-956 ancient relic setup flow",
        EventId::Nonupeipe => "EVENT:NONUPEIPE FurCoat.AfterObtained AddMarkedRooms map/RNG continuation",
        EventId::Pael => "EVENT:PAEL r7 shared.py:1360-1394 ancient relic offer flow",
        EventId::Tanx => "EVENT:TANX r7 shared.py:1919-1954 ancient relic offer flow",
        EventId::Tezcatara => "EVENT:TEZCATARA r7 shared.py:1962-1991 ancient relic offer flow",
        EventId::Neow => "EVENT:NEOW ancient map entry uses dedicated Neow reward flow",
        _ => "EVENT:unknown catalog event",
    }
}

/// Build the source-defined initial state for an event implemented by this packet.
/// `None` is deliberate fail-closed routing for an event whose run-layer behavior
/// still depends on an unported engine feature.
pub(crate) fn initialize(
    id: crate::identity::EventId,
    run_seed: u64,
    gold: i32,
    rest_heal_amount: i32,
    hp: i32,
    max_hp: i32,
) -> Option<EventState> {
    let mut event = EventState::new(
        id,
        Rng::new(run_seed.wrapping_add(crate::rng::deterministic_hash64(id.name()))),
    );
    match id {
        EventId::TinkerTime => { tinker_time::initialize(&mut event); Some(event) }
        EventId::Trial => { trial::initialize(&mut event); Some(event) }
        EventId::BattlewornDummy => {
            event.options.replace([
                EventOption::new("setting_1", "Setting 1", "Defeat in 3 turns for a potion"),
                EventOption::new("setting_2", "Setting 2", "Defeat in 3 turns to upgrade 2 cards"),
                EventOption::new("setting_3", "Setting 3", "Defeat in 3 turns for a relic"),
            ]);
            Some(event)
        }
        EventId::Reflections => {
            event.options.replace([
                EventOption::new("touch_a_mirror", "Touch a Mirror", "Downgrade 2, upgrade 4 random cards"),
                EventOption::new("shatter", "Shatter", "Duplicate the deck, gain Bad Luck"),
            ]);
            Some(event)
        }
        EventId::WelcomeToWongos | EventId::FakeMerchant | EventId::RanwidTheElder | EventId::RelicTrader | EventId::Nonupeipe => Some(event), // Owner inventory and bag initialization in RunManager.
        EventId::Vakuu => {
            // Vakuu.GenerateInitialOptions: shuffle ALL three pools in order;
            // choosing one item directly would consume only three draws, not seven.
            let pools = [
                vec![(RelicId::BloodSoakedRose, "Blood-Soaked Rose"),
                    (RelicId::WhisperingEarring, "Whispering Earring"), (RelicId::Fiddle, "Fiddle")],
                vec![(RelicId::PreservedFog, "Preserved Fog"), (RelicId::SereTalon, "Sere Talon"),
                    (RelicId::DistinguishedCape, "Distinguished Cape")],
                vec![(RelicId::ChoicesParadox, "Choices Paradox"), (RelicId::MusicBox, "Music Box"),
                    (RelicId::LordsParasol, "Lord's Parasol"), (RelicId::JeweledMask, "Jeweled Mask")],
            ];
            for (option, mut pool) in ["relic_1", "relic_2", "relic_3"].into_iter().zip(pools) {
                event.rng.shuffle(&mut pool);
                event.relics.push(pool[0].0);
                // Frozen policy UI text; the Sere Talon pickup still applies
                // its C# max-HP loss in the gameplay continuation.
                event.options.push(EventOption::new(option, pool[0].1, "Gain a relic"));
            }
            Some(event)
        }
        EventId::StoneOfAllTime => Some(event), // Owner-dependent initialization in RunManager.
        EventId::EndlessConveyor => Some(event),
        EventId::SlipperyBridge => Some(event),
        EventId::PunchOff => {
            // C# PunchOff.CalculateVars draws even though neither option uses gold.
            event.ints[0] = event.rng.next_int_exclusive(91, 99) as i32;
            event.options.push(EventOption::new("nab", "Nab", "Gain Injury curse + relic reward"));
            event.options.push(EventOption::new("take_them", "I Can Take Them", "Fight for relic + potion"));
            Some(event)
        }
        EventId::CrystalSphere => {
            // Frozen r7 CrystalSphere.calculate_vars consumes one event-RNG draw
            // before exposing the two initial options.
            event.ints[0] = 50 + event.rng.next_int_exclusive(1, 50) as i32;
            event.options.push(EventOption::new(
                "pay",
                "Uncover Future",
                "3 Prophesize picks",
            ));
            event.options.push(EventOption::new(
                "debt",
                "Payment Plan",
                "6 picks, gain Debt curse",
            ));
            Some(event)
        }
        EventId::DollRoom => {
            event.options.push(EventOption::new(
                "random",
                "Random",
                "Get a random doll relic",
            ));
            event.options.push(EventOption::new(
                "take_time",
                "Take Some Time",
                "Take 5 damage, choose from 2 dolls",
            ));
            event.options.push(EventOption::new(
                "examine",
                "Examine",
                "Take 15 damage, choose from 3 dolls",
            ));
            Some(event)
        }
        EventId::FieldOfManSizedHoles => {
            event.options.push(EventOption::new(
                "resist",
                "Resist",
                "Remove 2 cards, gain Normality curse",
            ));
            event.options.push(EventOption::new(
                "enter",
                "Enter Your Hole",
                "Enchant a card with Perfect Fit",
            ));
            Some(event)
        }
        EventId::InfestedAutomaton => {
            event.options.push(EventOption::new(
                "study",
                "Study",
                "Gain a random Power card",
            ));
            event.options.push(EventOption::new(
                "touch_core",
                "Touch Core",
                "Gain a random 0-cost card",
            ));
            Some(event)
        }
        EventId::SpiritGrafter => {
            event.options.push(EventOption::new(
                "let_it_in",
                "Let It In",
                "Heal 25 HP, gain Metamorphosis card",
            ));
            event.options.push(EventOption::new(
                "rejection",
                "Rejection",
                "Upgrade 1 card, take 10 damage",
            ));
            Some(event)
        }
        EventId::TheLanternKey => {
            event.options.push(EventOption::new(
                "return_key",
                "Return the Key",
                "Gain 100 gold",
            ));
            event.options.push(EventOption::new(
                "keep_key",
                "Keep the Key",
                "Fight for Lantern Key card",
            ));
            Some(event)
        }
        // RunManager fills this table from the live character pool.
        EventId::ColorfulPhilosophers => Some(event),
        EventId::WhisperingHollow => {
            event.ints[0] = 35 + event.rng.next_int_exclusive(-9, 10) as i32;
            event.options.push(EventOption::new("gold", "Pay Gold", "Gain 2 potions"));
            event.options.push(EventOption::new("hug", "Hug", "Take 9 damage, transform 1 card"));
            Some(event)
        }
        EventId::JungleMazeAdventure => {
            // JungleMazeAdventure.cs:66-69; truncate only after adding variance.
            let solo = 150.0 + event.rng.next_float_range(-15.0, 15.0);
            let join = 50.0 + event.rng.next_float_range(-15.0, 15.0);
            event.ints[0] = solo as i32;
            event.ints[1] = join as i32;
            event.options.push(EventOption::new("solo", "Solo Quest", "")
                .with_dynamic_description(EventDescription::JungleSoloGold(solo.to_bits())));
            event.options.push(EventOption::new("join", "Join Forces", "")
                .with_dynamic_description(EventDescription::JungleJoinGold(join.to_bits())));
            Some(event)
        }
        EventId::SapphireSeed => {
            event.options.push(EventOption::new("eat", "Eat", "Heal 9 HP, upgrade 1 card"));
            event.options.push(EventOption::new("plant", "Plant the Seed", "Enchant a card with Sown"));
            Some(event)
        }
        EventId::TabletOfTruth => {
            event.ints[1] = 3;
            event.options.push(EventOption::new("decipher", "Decipher", "")
                .with_dynamic_description(EventDescription::TabletDecipherCost(3)));
            event.options.push(EventOption::new("smash", "Smash", "Heal 20 HP"));
            Some(event)
        }
        EventId::LuminousChoir => {
            // C# LuminousChoir.CalculateVars: one event-local NextInt(0, 50).
            event.ints[0] = 149 - event.rng.next_int_exclusive(0, 50) as i32;
            event.options.push(EventOption::new("reach", "Reach Into the Flesh", "Remove 2 cards, gain Spore Mind curse"));
            let mut tribute = EventOption::new("tribute", "Offer Tribute", "Gain a relic");
            tribute.enabled = gold >= event.ints[0];
            event.options.push(tribute);
            Some(event)
        }
        EventId::BrainLeech => {
            event.options.push(EventOption::new("share_knowledge", "Share Knowledge", "Pick 1 of 5 character cards to gain"));
            event.options.push(EventOption::new("rip", "Rip", "Take 5 damage, gain a colorless card"));
            Some(event)
        }
        EventId::SelfHelpBook => {
            event.options.push(EventOption::new("read_back", "Read the Back", "Enchant an Attack with Sharp +2"));
            event.options.push(EventOption::new("read_passage", "Read a Passage", "Enchant a Skill with Nimble +2"));
            event.options.push(EventOption::new("read_entire", "Read Entire Book", "Enchant a Power with Swift +2"));
            Some(event)
        }
        EventId::RoomFullOfCheese => {
            // C# RoomFullOfCheese.GenerateInitialOptions; Gorge is an eight-card grid.
            event.options.push(EventOption::new("gorge", "Gorge", "Pick 2 of 8 common cards"));
            event.options.push(EventOption::new("search", "Search",
                "Take 14 damage, gain Chosen Cheese relic"));
            Some(event)
        }
        // C# ThisOrThat.CalculateVars/GenerateInitialOptions: one event-RNG draw,
        // shared by the displayed gold amount and Plain's eventual payout.
        EventId::ThisOrThat => {
            event.ints[0] = event.rng.next_int_exclusive(41, 69) as i32;
            event.options.push(
                EventOption::new("plain", "Plain", "")
                    .with_dynamic_description(EventDescription::ThisOrThatPlainGold(event.ints[0])),
            );
            event.options.push(EventOption::new(
                "ornate", "Ornate", "Gain a relic + Clumsy curse",
            ));
            Some(event)
        }
        EventId::RoundTeaParty => {
            event.options.push(EventOption::new("enjoy_tea","Enjoy Tea","Gain Royal Poison and heal to full"));
            event.options.push(EventOption::new("pick_fight","Pick a Fight","Take 11 damage, gain a relic"));
            Some(event)
        }
        EventId::Amalgamator => {
            event.options.push(EventOption::new(
                "combine_strikes",
                "Combine Strikes",
                "Remove 2 Strikes, gain Ultimate Strike",
            ));
            event.options.push(EventOption::new(
                "combine_defends",
                "Combine Defends",
                "Remove 2 Defends, gain Ultimate Defend",
            ));
            Some(event)
        }
        EventId::AromaOfChaos => {
            event
                .options
                .push(EventOption::new("let_go", "Let Go", "Transform 1 card"));
            event.options.push(EventOption::new(
                "maintain_control",
                "Maintain Control",
                "Upgrade 1 card",
            ));
            Some(event)
        }
        EventId::AbyssalBaths => {
            event.ints[0] = 3; // AbyssalBaths._damage
            event.options.push(EventOption::new(
                "immerse",
                "Immerse",
                abyssal_damage_description(3),
            ));
            event
                .options
                .push(EventOption::new("abstain", "Abstain", "Heal 10 HP"));
            Some(event)
        }
        EventId::WaterloggedScriptorium => {
            event.options.push(EventOption::new(
                "bloody_ink",
                "Bloody Ink",
                "Gain 6 Max HP",
            ));
            event.options.push(EventOption {
                option_id: "tentacle_quill",
                label: if gold >= 55 {
                    "Tentacle Quill (55g)"
                } else {
                    "Tentacle Quill"
                },
                description: "Enchant 1 card with Steady",
                dynamic_description: None,
                enabled: gold >= 55,
            });
            event.options.push(EventOption {
                option_id: "prickly_sponge",
                label: if gold >= 99 {
                    "Prickly Sponge (99g)"
                } else {
                    "Prickly Sponge"
                },
                description: "Enchant 2 cards with Steady",
                dynamic_description: None,
                enabled: gold >= 99,
            });
            Some(event)
        }
        EventId::DenseVegetation => {
            event.ints[0] = event.rng.next_int_exclusive(61, 100) as i32;
            event.ints[1] = rest_heal_amount;
            event.options.push(
                EventOption::new("trudge_on", "Trudge On", "").with_dynamic_description(
                    EventDescription::DenseVegetationTrudgeGold(event.ints[0]),
                ),
            );
            event.options.push(
                EventOption::new("rest", "Rest", "").with_dynamic_description(
                    EventDescription::DenseVegetationRestHeal(event.ints[1]),
                ),
            );
            Some(event)
        }
        EventId::DoorsOfLightAndDark => {
            event.options.push(EventOption::new(
                "light",
                "Door of Light",
                "Upgrade 2 random cards",
            ));
            event.options.push(EventOption::new(
                "dark",
                "Door of Dark",
                "Remove 1 card",
            ));
            Some(event)
        }
        EventId::DrowningBeacon => {
            event.options.push(EventOption::new(
                "bottle",
                "Bottle the Water",
                "Gain Glowwater Potion",
            ));
            event.options.push(EventOption::new(
                "climb",
                "Climb",
                "Lose 13 Max HP, gain Fresnel Lens relic",
            ));
            Some(event)
        }
        EventId::GraveOfTheForgotten => {
            event.options.push(EventOption::new(
                "confront",
                "Confront",
                "Gain Decay curse, enchant a card",
            ));
            event.options.push(EventOption::new(
                "accept",
                "Accept",
                "Gain Forgotten Soul relic",
            ));
            Some(event)
        }
        // events/shared.py:832-895. `calculate_vars` for ColossalFlower is state-local;
        // no RNG is consumed until the player chooses to reach deeper.
        EventId::ColossalFlower => {
            event.options.push(
                EventOption::new("extract", "Extract Prize", "")
                    .with_dynamic_description(EventDescription::ColossalFlowerExtractGold(35)),
            );
            event.options.push(
                EventOption::new("reach_deeper", "Reach Deeper", "")
                    .with_dynamic_description(EventDescription::ColossalFlowerReachDeeperDamage(5)),
            );
            Some(event)
        }
        // events/shared.py:1780-1829. The two rolls happen when event vars are calculated,
        // before the options are exposed, and use the event's own RNG stream.
        EventId::SunkenTreasury => {
            event.ints[0] = 60 + event.rng.next_int_exclusive(0, 16) as i32 - 8;
            event.ints[1] = 333 + event.rng.next_int_exclusive(0, 61) as i32 - 30;
            event.options.push(
                EventOption::new("first_chest", "Small Chest", "").with_dynamic_description(
                    EventDescription::SunkenTreasurySmallGold(event.ints[0]),
                ),
            );
            event.options.push(
                EventOption::new("second_chest", "Large Chest", "").with_dynamic_description(
                    EventDescription::SunkenTreasuryLargeGold(event.ints[1]),
                ),
            );
            Some(event)
        }
        // events/shared.py:1735-1777. The gold variance is rolled once on entry.
        EventId::SunkenStatue => {
            event.ints[0] = 111 + event.rng.next_int_exclusive(-10, 11) as i32;
            event.options.push(EventOption::new(
                "grab_sword",
                "Grab the Sword",
                "Gain Sword of Stone relic",
            ));
            event.options.push(
                EventOption::new("dive", "Dive Into Water", "")
                    .with_dynamic_description(EventDescription::SunkenStatueDive(event.ints[0])),
            );
            Some(event)
        }
        // events/shared.py:2165-2206. The reward lists are ordered exactly as in r7.
        EventId::TrashHeap => {
            event.options.push(EventOption::new(
                "dive_in",
                "Dive In",
                "Take 8 damage, gain a relic",
            ));
            event
                .options
                .push(EventOption::new("grab", "Grab", "Gain 100 gold + a card"));
            Some(event)
        }
        // C# Bugslayer.AddAndPreview: add the selected fixed card to the deck.
        EventId::Bugslayer => {
            event.options.push(EventOption::new(
                "exterminate",
                "Extermination",
                "Gain Exterminate card",
            ));
            event
                .options
                .push(EventOption::new("squash", "Squash", "Gain Squash card"));
            Some(event)
        }
        // events/shared.py:752-776. Eat applies GainMaxHp(7); taking the egg adds the card.
        EventId::ByrdonisNest => {
            event
                .options
                .push(EventOption::new("eat", "Eat", "+7 Max HP"));
            event.options.push(EventOption::new(
                "take",
                "Take the Egg",
                "Gain Byrdonis Egg card",
            ));
            Some(event)
        }
        // relics/shop_event.py:1036-1053, 1884-1907. The relic pickup hooks are represented
        // as ordered run effects when the option is resolved.
        EventId::HungryForMushrooms => {
            event.options.push(EventOption::new(
                "big",
                "Big Mushroom",
                "Gain Big Mushroom relic (+20 Max HP)",
            ));
            event.options.push(EventOption::new(
                "fragrant",
                "Fragrant Mushroom",
                "Take 15 damage, upgrade 2 random cards, gain Fragrant Mushroom relic",
            ));
            Some(event)
        }
        // events/shared.py:1260-1305. calculate_vars consumes one event-RNG draw on entry.
        EventId::LostWisp => {
            event.ints[0] = 60 + event.rng.next_int_exclusive(-15, 16) as i32;
            event.options.push(EventOption::new(
                "claim",
                "Claim",
                "Gain Lost Wisp relic + Decay curse",
            ));
            event.options.push(
                EventOption::new("search", "Search", "")
                    .with_dynamic_description(EventDescription::LostWispSearchGold(event.ints[0])),
            );
            Some(event)
        }
        EventId::MorphicGrove => {
            event.options.push(EventOption::new(
                "group",
                "Group",
                "Lose all gold, transform 2 cards",
            ));
            event
                .options
                .push(EventOption::new("loner", "Loner", "Gain 5 Max HP"));
            Some(event)
        }
        EventId::PotionCourier => {
            event.options.push(EventOption::new(
                "grab",
                "Grab Potions",
                "Gain 3 Foul Potions",
            ));
            event.options.push(EventOption::new(
                "ransack",
                "Ransack",
                "Gain 1 uncommon potion",
            ));
            Some(event)
        }
        EventId::SpiralingWhirlpool => {
            event.ints[0] = max_hp * 33 / 100;
            event.options.push(EventOption::new(
                "observe",
                "Observe the Spiral",
                "Enchant a card with Spiral",
            ));
            event.options.push(
                EventOption::new("drink", "Drink", "").with_dynamic_description(
                    EventDescription::SpiralingWhirlpoolHeal(event.ints[0]),
                ),
            );
            Some(event)
        }
        EventId::Symbiote => {
            event.options.push(EventOption::new(
                "approach",
                "Approach",
                "Enchant a card with Corrupted",
            ));
            event.options.push(EventOption::new(
                "kill_fire",
                "Kill with Fire",
                "Transform 1 card",
            ));
            Some(event)
        }
        // events/act1.py:199-244. Event eligibility requires at least 150 gold.
        EventId::TeaMaster => {
            event.options.push(EventOption {
                option_id: "bone_tea",
                label: if gold >= 50 { "Bone Tea (50g)" } else { "Bone Tea" },
                description: "Gain Bone Tea relic",
                dynamic_description: None,
                enabled: gold >= 50,
            });
            event.options.push(EventOption {
                option_id: "ember_tea",
                label: if gold >= 150 { "Ember Tea (150g)" } else { "Ember Tea" },
                description: "Gain Ember Tea relic",
                dynamic_description: None,
                enabled: gold >= 150,
            });
            event.options.push(EventOption::new(
                "discourtesy",
                "Tea of Discourtesy",
                "Free: gain Tea of Discourtesy relic",
            ));
            Some(event)
        }
        // events/act1.py:154-194. Potion reward generation is deferred to RunManager.
        EventId::TheLegendsWereTrue => {
            event.options.push(EventOption::new(
                "nab_map",
                "Nab the Map",
                "Gain Spoils Map card",
            ));
            event.options.push(EventOption::new(
                "find_exit",
                "Slowly Find an Exit",
                "Take 8 damage, gain a potion",
            ));
            Some(event)
        }
        // events/shared.py:2683-2724. Potion and card-removal followups are run-layer actions.
        EventId::Wellspring => {
            event.options.push(EventOption::new(
                "bottle",
                "Bottle",
                "Gain a random potion",
            ));
            event.options.push(EventOption::new(
                "bathe",
                "Bathe",
                "Remove 1 card, gain Guilty curse",
            ));
            Some(event)
        }
        // events/shared.py:2585-2636. Event eligibility is enforced by the selector.
        EventId::UnrestSite => {
            event.ints[0] = (max_hp - hp).max(0);
            event.options.push(
                EventOption::new("rest", "Rest", "").with_dynamic_description(
                    EventDescription::UnrestSiteHeal(event.ints[0]),
                ),
            );
            event.options.push(EventOption::new(
                "kill",
                "Kill",
                "Lose 8 Max HP, gain a relic",
            ));
            Some(event)
        }
        EventId::WoodCarvings => {
            event.options.push(EventOption::new(
                "bird",
                "Bird",
                "Transform a Basic card into Peck",
            ));
            event.options.push(EventOption::new(
                "snake",
                "Snake",
                "Enchant a card with Slither",
            ));
            event.options.push(EventOption::new(
                "torus",
                "Torus",
                "Transform a Basic card into Toric Toughness",
            ));
            Some(event)
        }
        EventId::ZenWeaver => {
            event.options.push(EventOption {
                option_id: "breathing",
                label: "Breathing Techniques",
                description: "Pay 50g, gain 2 Enlightenment cards",
                dynamic_description: None,
                enabled: true,
            });
            event.options.push(EventOption {
                option_id: "emotional",
                label: "Emotional Awareness",
                description: "Pay 125g, remove 1 card",
                dynamic_description: None,
                enabled: gold >= 125,
            });
            event.options.push(EventOption {
                option_id: "acupuncture",
                label: "Arachnid Acupuncture",
                description: "Pay 250g, remove 2 cards",
                dynamic_description: None,
                enabled: gold >= 250,
            });
            Some(event)
        }
        _ => None,
    }
}

/// Resolve event-local logic. Run ownership effects are returned to `RunManager`
/// so this module stays independent of the run manager's storage layout.
pub(crate) fn roll_trash_relic(event: &mut EventState) -> RelicId {
    // C# TrashHeap.DiveIn rolls only after the damage task has completed.
    const RELICS: &[RelicId] = &[
        RelicId::DarkstonePeriapt, RelicId::DreamCatcher, RelicId::HandDrill,
        RelicId::MawBank, RelicId::TheBoot,
    ];
    RELICS[event.rng.choice_index(RELICS.len())]
}

pub(crate) fn choose<S: native_identity::EventOptionSelector>(event: &mut EventState, option_id: S) -> Option<EventResolution> {
    let option_id = option_id.resolve(event);
    use crate::identity::EventOptionId as O;
    match event.id {
        // C# ThisOrThat.Plain damages before granting gold.
        EventId::ThisOrThat if option_id.is(O::ThisOrThatInitialPlain) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseHp(6));
            out.effects.push(EventEffect::GainGold(event.ints[0]));
            Some(out)
        }
        EventId::AbyssalBaths => {
            if option_id.is(O::AbyssalBathsInitialAbstain) || option_id.is(O::AbyssalBathsAllExitBaths) {
                let mut out = EventResolution::finished();
                if option_id.is(O::AbyssalBathsInitialAbstain) {
                    out.effects.push(EventEffect::Heal(10));
                }
                return Some(out);
            }
            if !option_id.is(O::AbyssalBathsInitialImmerse) && !option_id.is(O::AbyssalBathsAllLinger) {
                return None;
            }
            if option_id.is(O::AbyssalBathsAllLinger) {
                event.ints[1] = (event.ints[1] + 1).min(9);
            }
            event.options.set_page(if option_id.is(O::AbyssalBathsInitialImmerse) { EventPageId::AbyssalBathsImmerse }
                else { EventPageId::AbyssalBathsLingerLingerCount }, if option_id.is(O::AbyssalBathsInitialImmerse) {
                    EventParameter::None
                } else { EventParameter::Counter(event.ints[1]) });
            let damage = event.ints[0];
            event.ints[0] = damage + 1;
            let mut out = EventResolution::continuing();
            out.effects.push(EventEffect::GainMaxHp(2));
            out.effects.push(EventEffect::LoseHp(damage));
            out.next_options.push(EventOption::new(
                "linger",
                "Linger",
                abyssal_damage_description(event.ints[0]),
            ));
            out.next_options.push(EventOption::new(
                "exit_baths",
                "Exit Baths",
                "Leave the baths",
            ));
            Some(out)
        }
        EventId::WaterloggedScriptorium if option_id.is(O::WaterloggedScriptoriumInitialBloodyInk) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainMaxHp(6));
            Some(out)
        }
        EventId::DrowningBeacon if option_id.is(O::DrowningBeaconInitialClimb) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseMaxHp(13));
            out.effects.push(EventEffect::AddRelic(RelicId::FresnelLens));
            Some(out)
        }
        EventId::GraveOfTheForgotten if option_id.is(O::GraveOfTheForgottenInitialAccept) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddRelic(RelicId::ForgottenSoul));
            Some(out)
        }
        EventId::DenseVegetation if option_id.is(O::DenseVegetationInitialTrudgeOn) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseHp(8));
            out.effects.push(EventEffect::GainGold(event.ints[0]));
            Some(out)
        }
        EventId::DenseVegetation if option_id.is(O::DenseVegetationInitialRest) => {
            event.options.set_page(EventPageId::DenseVegetationRest, EventParameter::None);
            let mut out = EventResolution::continuing();
            out.effects.push(EventEffect::Heal(event.ints[1]));
            out.next_options.push(EventOption::new(
                "fight",
                "Fight",
                "Fight through the vegetation",
            ));
            Some(out)
        }
        EventId::ColossalFlower if matches!(option_id.id,
            O::ColossalFlowerInitialExtractCurrentPrizeNumberOfDigs1 |
            O::ColossalFlowerReachDeeperNumberOfDigsExtractCurrentPrizeNumberOfDigs1 |
            O::ColossalFlowerReachDeeper2ExtractInstead) => {
            let gold = [35, 75, 135][event.ints[0].clamp(0, 2) as usize];
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(gold));
            Some(out)
        }
        EventId::ColossalFlower if matches!(option_id.id,
            O::ColossalFlowerInitialReachDeeperNumberOfDigs1 |
            O::ColossalFlowerReachDeeperNumberOfDigsReachDeeperNumberOfDigs1) => {
            let depth = event.ints[0].clamp(0, 2) as usize;
            let damage = [5, 6, 7][depth];
            let mut out = EventResolution::continuing();
            out.effects.push(EventEffect::LoseHp(damage));
            event.ints[0] = (depth + 1).min(2) as i32;
            event.options.set_page(if event.ints[0] == 2 { EventPageId::ColossalFlowerReachDeeper2 }
                else { EventPageId::ColossalFlowerReachDeeperNumberOfDigs }, if event.ints[0] == 2 {
                    EventParameter::None
                } else { EventParameter::Counter(event.ints[0]) });
            if event.ints[0] < 2 {
                let next_gold = [75, 135][depth];
                let next_damage = [6, 7][depth];
                out.next_options.push(
                    EventOption::new("extract", "Extract Prize", "").with_dynamic_description(
                        EventDescription::ColossalFlowerExtractGold(next_gold),
                    ),
                );
                out.next_options.push(
                    EventOption::new("reach_deeper", "Reach Deeper", "").with_dynamic_description(
                        EventDescription::ColossalFlowerReachDeeperDamage(next_damage),
                    ),
                );
            } else {
                out.next_options.push(
                    EventOption::new("extract", "Extract Prize", "")
                        .with_dynamic_description(EventDescription::ColossalFlowerExtractGold(135)),
                );
                out.next_options.push(
                    EventOption::new("pollinous_core", "Pollinous Core", "")
                        .with_dynamic_description(
                            EventDescription::ColossalFlowerPollinousCoreDamage(7),
                        ),
                );
            }
            Some(out)
        }
        EventId::ColossalFlower if option_id.is(O::ColossalFlowerReachDeeper2PollinousCore) => {
            let relic = RelicId::from_name("POLLINOUS_CORE")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseHp(7));
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::SunkenTreasury if option_id.is(O::SunkenTreasuryInitialFirstChest) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(event.ints[0]));
            Some(out)
        }
        EventId::SunkenTreasury if option_id.is(O::SunkenTreasuryInitialSecondChest) => {
            let greed = CardId::from_name("GREED")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(event.ints[1]));
            out.effects.push(EventEffect::AddCard(greed));
            Some(out)
        }
        EventId::SunkenStatue if option_id.is(O::SunkenStatueInitialGrabSword) => {
            let relic = RelicId::from_name("SWORD_OF_STONE")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::SunkenStatue if option_id.is(O::SunkenStatueInitialDiveIntoWater) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(event.ints[0]));
            out.effects.push(EventEffect::LoseHp(7));
            Some(out)
        }
        EventId::TrashHeap if option_id.is(O::TrashHeapInitialDiveIn) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseHp(8));
            out.effects.push(EventEffect::RollTrashRelic);
            Some(out)
        }
        EventId::TrashHeap if option_id.is(O::TrashHeapInitialGrab) => {
            const CARDS: &[&str] = &[
                "CALTROPS",
                "CLASH",
                "DISTRACTION",
                "DUAL_WIELD",
                "ENTRENCH",
                "HELLO_WORLD_CARD",
                "OUTMANEUVER",
                "REBOUND",
                "RIP_AND_TEAR",
                "STACK",
            ];
            let name = CARDS[event.rng.choice_index(CARDS.len())];
            let card = CardId::from_name(name)?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(100));
            out.effects.push(EventEffect::AddCard(card));
            Some(out)
        }
        EventId::Bugslayer if matches!(option_id.id,
            O::BugslayerInitialExtermination | O::BugslayerInitialSquash) => {
            let name = if option_id.is(O::BugslayerInitialExtermination) {
                "EXTERMINATE"
            } else {
                "SQUASH"
            };
            let card = CardId::from_name(name)?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddCard(card));
            Some(out)
        }
        EventId::ByrdonisNest if option_id.is(O::ByrdonisNestInitialEat) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainMaxHp(7));
            Some(out)
        }
        EventId::ByrdonisNest if option_id.is(O::ByrdonisNestInitialTake) => {
            let card = CardId::from_name("BYRDONIS_EGG")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddCard(card));
            Some(out)
        }
        EventId::HungryForMushrooms if option_id.parameter == EventParameter::Relic(RelicId::BigMushroom) => {
            let relic = RelicId::from_name("BIG_MUSHROOM")?;
            let mut out = EventResolution::finished();
            // BigMushroom.after_obtained adds 20 Max HP and heals by the same amount.
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::HungryForMushrooms if option_id.parameter == EventParameter::Relic(RelicId::FragrantMushroom) => {
            let relic = RelicId::from_name("FRAGRANT_MUSHROOM")?;
            let mut out = EventResolution::finished();
            // FragrantMushroom.after_obtained takes damage before upgrading two cards.
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::LostWisp if option_id.is(O::LostWispInitialClaim) => {
            let relic = RelicId::from_name("LOST_WISP")?;
            let curse = CardId::from_name("DECAY")?;
            let mut out = EventResolution::finished();
            // C# LostWisp.Claim adds the curse before obtaining the relic.
            out.effects.push(EventEffect::AddCard(curse));
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::LostWisp if option_id.is(O::LostWispInitialSearch) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainGold(event.ints[0]));
            Some(out)
        }
        EventId::MorphicGrove if option_id.is(O::MorphicGroveInitialLoner) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::GainMaxHp(5));
            Some(out)
        }
        EventId::SpiralingWhirlpool if option_id.is(O::SpiralingWhirlpoolInitialDrink) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::Heal(event.ints[0]));
            Some(out)
        }
        EventId::TeaMaster if option_id.is(O::TeaMasterInitialBoneTea) => {
            let relic = RelicId::from_name("BONE_TEA")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseGold(50));
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::TeaMaster if option_id.is(O::TeaMasterInitialEmberTea) => {
            let relic = RelicId::from_name("EMBER_TEA")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseGold(150));
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::TeaMaster if option_id.is(O::TeaMasterInitialTeaOfDiscourtesy) => {
            let relic = RelicId::from_name("TEA_OF_DISCOURTESY")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddRelic(relic));
            Some(out)
        }
        EventId::TheLegendsWereTrue if option_id.is(O::TheLegendsWereTrueInitialNabTheMap) => {
            let card = CardId::from_name("SPOILS_MAP")?;
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::AddCard(card));
            Some(out)
        }
        EventId::TheLegendsWereTrue if option_id.is(O::TheLegendsWereTrueInitialSlowlyFindAnExit) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseHp(8));
            Some(out)
        }
        EventId::UnrestSite if option_id.is(O::UnrestSiteInitialRest) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::Heal(event.ints[0]));
            out.effects.push(EventEffect::AddCard(CardId::from_name("POOR_SLEEP")?));
            Some(out)
        }
        EventId::UnrestSite if option_id.is(O::UnrestSiteInitialKill) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseMaxHp(8));
            Some(out)
        }
        EventId::ZenWeaver if option_id.is(O::ZenWeaverInitialBreathingTechniques) => {
            let mut out = EventResolution::finished();
            out.effects.push(EventEffect::LoseGold(50));
            out.effects.push(EventEffect::AddCard(CardId::Enlightenment));
            out.effects.push(EventEffect::AddCard(CardId::Enlightenment));
            Some(out)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RunChoiceEffect {
    StorePaelsTooth,
    RemoveReward { owner: crate::card::CId, reward_index: usize },
    AddCards,
    Duplicate,
    Upgrade,
    UpgradeAndDamage(i32),
    Remove { damage_after: i32 },
    RemoveAndLoseGold(i32),
    RemoveAndAddCard(CardId),
    TransformUpgrade,
    TransformMaul,
    Transform,
    TransformEventRng,
    TransformEventRngAndDamage(i32),
    Enchant { name: crate::identity::EnchantmentId, amount: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChoiceResume {
    MapChoice,
    Event,
    Shop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunChoiceCard {
    pub instance_id: u32,
    pub card_id: CardId,
    pub source_pile: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct RunCardChoice {
    /// Stable named request site. Constructors must supply a nonempty ID.
    pub wire_source: Option<&'static str>,
    pub prompt: &'static str,
    pub options: Vec<RunChoiceCard>,
    pub selected: Vec<usize>,
    pub min_choices: usize,
    pub max_choices: usize,
    pub allow_skip: bool,
    pub single_select: bool,
    pub effect: RunChoiceEffect,
    pub resume: ChoiceResume,
    pub reward_cards: Vec<crate::card::Card>,
}

impl RunCardChoice {
    #[inline]
    pub fn is_multi(&self) -> bool {
        matches!(self.effect, RunChoiceEffect::RemoveReward { .. })
            || (!self.single_select && (self.min_choices != 1 || self.max_choices != 1))
    }

    #[inline]
    pub fn can_confirm(&self) -> bool {
        self.selected.len() >= self.min_choices || (self.allow_skip && self.selected.is_empty())
    }

    #[inline]
    pub fn can_toggle(&self, index: usize) -> bool {
        self.selected.contains(&index) || self.selected.len() < self.max_choices
    }

    pub fn toggle(&mut self, index: usize) -> bool {
        if index >= self.options.len() {
            return false;
        }
        if let Some(pos) = self.selected.iter().position(|x| *x == index) {
            self.selected.remove(pos);
            return true;
        }
        if self.selected.len() >= self.max_choices {
            return false;
        }
        self.selected.push(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{EventDescription, EventEffect, EventId, choose, initialize};
    use crate::tables::{CardId, RelicId};

    #[test]
    fn this_or_that_rolls_gold_once_and_plain_damages_before_payout() {
        for seed in 0..32 {
            let mut event = initialize(EventId::ThisOrThat, seed, 99, 24, 50, 100).unwrap();
            let gold = event.ints[0];
            assert!((41..69).contains(&gold)); // C# ThisOrThat.CalculateVars.
            assert_eq!(event.rng.counter, 1);
            assert_eq!(event.options[0].option_id, "plain");
            assert_eq!(event.options[1].option_id, "ornate");
            assert_eq!(
                event.options[0].dynamic_description.unwrap().render(),
                format!("Take 6 damage, gain {gold} gold"),
            );
            assert_eq!(
                choose(&mut event, "plain").unwrap().effects.as_slice(),
                &[EventEffect::LoseHp(6), EventEffect::GainGold(gold)],
            );
            assert_eq!(event.rng.counter, 1);
        }
    }

    #[test]
    fn colossal_flower_tracks_each_dig_and_terminal_relic_choice() {
        let mut event = initialize(EventId::ColossalFlower, 17, 99, 24, 50, 100).unwrap();
        assert_eq!(
            event.options[0].dynamic_description.unwrap().render(),
            "Gain 35 gold"
        );
        assert_eq!(
            event.options[1].dynamic_description.unwrap().render(),
            "Take 5 damage for better reward"
        );

        let first = choose(&mut event, "reach_deeper").unwrap();
        assert_eq!(first.effects.as_slice(), &[EventEffect::LoseHp(5)]);
        event.options.replace(first.next_options);
        assert_eq!(
            event.options[0].dynamic_description,
            Some(EventDescription::ColossalFlowerExtractGold(75))
        );
        assert_eq!(
            event.options[1].dynamic_description,
            Some(EventDescription::ColossalFlowerReachDeeperDamage(6))
        );

        let second = choose(&mut event, "reach_deeper").unwrap();
        assert_eq!(second.effects.as_slice(), &[EventEffect::LoseHp(6)]);
        event.options.replace(second.next_options);
        assert_eq!(
            event.options[0].dynamic_description,
            Some(EventDescription::ColossalFlowerExtractGold(135))
        );
        assert_eq!(
            event.options[1].dynamic_description,
            Some(EventDescription::ColossalFlowerPollinousCoreDamage(7))
        );

        let final_choice = choose(&mut event, "pollinous_core").unwrap();
        assert!(final_choice.finished);
        assert_eq!(final_choice.effects[0], EventEffect::LoseHp(7));
        assert!(matches!(final_choice.effects[1], EventEffect::AddRelic(_)));
    }

    #[test]
    fn sunken_treasury_rolls_both_gold_values_before_choice() {
        let mut event = initialize(EventId::SunkenTreasury, 4100000, 99, 24, 50, 100).unwrap();
        let small_gold = event.ints[0];
        let large_gold = event.ints[1];
        assert!((52..=67).contains(&small_gold));
        assert!((303..=363).contains(&large_gold));
        assert_eq!(
            event.options[0].dynamic_description.unwrap().render(),
            format!("Gain {small_gold} gold")
        );
        assert_eq!(
            event.options[1].dynamic_description.unwrap().render(),
            format!("Gain {large_gold} gold + Greed curse")
        );
        let first = choose(&mut event, "first_chest").unwrap();
        assert_eq!(
            first.effects.as_slice(),
            &[EventEffect::GainGold(small_gold)]
        );
        let second = choose(&mut event, "second_chest").unwrap();
        assert_eq!(second.effects[0], EventEffect::GainGold(large_gold));
        assert!(matches!(second.effects[1], EventEffect::AddCard(_)));
    }

    #[test]
    fn sunken_statue_rolls_gold_on_entry_and_resolves_both_options() {
        let mut event = initialize(EventId::SunkenStatue, 4100000, 99, 24, 50, 100).unwrap();
        let gold = event.ints[0];
        assert!((101..=121).contains(&gold));
        assert_eq!(event.options[0].option_id, "grab_sword");
        assert_eq!(event.options[1].option_id, "dive");
        assert_eq!(
            event.options[1].dynamic_description.unwrap().render(),
            format!("Gain {gold} gold, take 7 damage")
        );
        let sword = choose(&mut event, "grab_sword").unwrap();
        assert!(sword.finished);
        assert!(matches!(sword.effects[0], EventEffect::AddRelic(_)));
        let dive = choose(&mut event, "dive").unwrap();
        assert_eq!(
            dive.effects.as_slice(),
            &[EventEffect::GainGold(gold), EventEffect::LoseHp(7)]
        );
    }

    #[test]
    fn trash_heap_draws_only_from_the_chosen_reward_list() {
        let mut event = initialize(EventId::TrashHeap, 4100000, 99, 24, 50, 100).unwrap();
        assert_eq!(event.options[0].option_id, "dive_in");
        assert_eq!(event.options[1].option_id, "grab");

        let mut dive = event.clone();
        let result = choose(&mut dive, "dive_in").unwrap();
        assert_eq!(result.effects[0], EventEffect::LoseHp(8));
        assert_eq!(result.effects[1], EventEffect::RollTrashRelic);
        assert_eq!(dive.rng.counter, 0); // No roll before the damage is applied.
        let relic = super::roll_trash_relic(&mut dive);
        assert!([RelicId::DarkstonePeriapt, RelicId::DreamCatcher, RelicId::HandDrill,
            RelicId::MawBank, RelicId::TheBoot].contains(&relic));
        assert_eq!(dive.rng.counter, 1);

        let result = choose(&mut event, "grab").unwrap();
        assert_eq!(result.effects[0], EventEffect::GainGold(100));
        assert!(matches!(result.effects[1], EventEffect::AddCard(_)));
        assert_eq!(event.rng.counter, 1);
    }

    #[test]
    fn bugslayer_and_byrdonis_nest_resolve_fixed_card_and_max_hp_rewards() {
        let mut bugslayer = initialize(EventId::Bugslayer, 4100000, 99, 24, 50, 100).unwrap();
        assert_eq!(bugslayer.options[0].option_id, "exterminate");
        assert_eq!(bugslayer.options[1].option_id, "squash");
        assert_eq!(
            choose(&mut bugslayer, "exterminate")
                .unwrap()
                .effects
                .as_slice(),
            &[EventEffect::AddCard(
                CardId::from_name("EXTERMINATE").unwrap()
            )]
        );
        assert_eq!(
            choose(&mut bugslayer, "squash").unwrap().effects.as_slice(),
            &[EventEffect::AddCard(CardId::from_name("SQUASH").unwrap())]
        );

        let mut nest = initialize(EventId::ByrdonisNest, 4100000, 99, 24, 50, 100).unwrap();
        assert_eq!(
            choose(&mut nest, "eat").unwrap().effects.as_slice(),
            &[EventEffect::GainMaxHp(7)]
        );
        assert_eq!(
            choose(&mut nest, "take").unwrap().effects.as_slice(),
            &[EventEffect::AddCard(
                CardId::from_name("BYRDONIS_EGG").unwrap()
            )]
        );
    }

    #[test]
    fn hungry_for_mushrooms_models_both_relic_pickup_hooks_in_source_order() {
        let mut event = initialize(EventId::HungryForMushrooms, 4100000, 99, 24, 50, 100).unwrap();
        let big = choose(&mut event, "big").unwrap();
        assert_eq!(
            big.effects.as_slice(),
            &[
                EventEffect::AddRelic(RelicId::from_name("BIG_MUSHROOM").unwrap()),
            ]
        );

        let fragrant = choose(&mut event, "fragrant").unwrap();
        assert_eq!(
            fragrant.effects.as_slice(),
            &[
                EventEffect::AddRelic(RelicId::from_name("FRAGRANT_MUSHROOM").unwrap()),
            ]
        );
    }

    #[test]
    fn lost_wisp_rolls_search_gold_on_entry_and_resolves_claim() {
        let mut event = initialize(EventId::LostWisp, 4100000, 99, 24, 50, 100).unwrap();
        let gold = event.ints[0];
        assert!((45..=75).contains(&gold));
        assert_eq!(event.rng.counter, 1);
        assert_eq!(
            event.options[1].dynamic_description.unwrap().render(),
            format!("Gain {gold} gold")
        );
        assert_eq!(
            choose(&mut event, "search").unwrap().effects.as_slice(),
            &[EventEffect::GainGold(gold)]
        );
        assert_eq!(
            choose(&mut event, "claim").unwrap().effects.as_slice(),
            &[
                EventEffect::AddCard(CardId::from_name("DECAY").unwrap()),
                EventEffect::AddRelic(RelicId::from_name("LOST_WISP").unwrap()),
            ]
        );
    }

    #[test]
    fn tea_master_applies_the_selected_cost_before_relic_obtainment() {
        let mut event = initialize(EventId::TeaMaster, 4100000, 150, 24, 50, 100).unwrap();
        assert!(event.options.iter().all(|option| option.enabled));
        assert_eq!(event.options[0].label, "Bone Tea (50g)");
        assert_eq!(event.options[1].label, "Ember Tea (150g)");
        assert_eq!(event.options[1].description, "Gain Ember Tea relic");
        assert_eq!(event.options[2].description, "Free: gain Tea of Discourtesy relic");
        let locked = initialize(EventId::TeaMaster, 4100000, 49, 24, 50, 100).unwrap();
        assert_eq!(locked.options[0].label, "Bone Tea");
        assert_eq!(locked.options[1].label, "Ember Tea");
        assert!(!locked.options[0].enabled && !locked.options[1].enabled);
        assert_eq!(
            choose(&mut event, "bone_tea").unwrap().effects.as_slice(),
            &[
                EventEffect::LoseGold(50),
                EventEffect::AddRelic(RelicId::from_name("BONE_TEA").unwrap()),
            ]
        );
        assert_eq!(
            choose(&mut event, "ember_tea").unwrap().effects.as_slice(),
            &[
                EventEffect::LoseGold(150),
                EventEffect::AddRelic(RelicId::from_name("EMBER_TEA").unwrap()),
            ]
        );
        assert_eq!(
            choose(&mut event, "discourtesy")
                .unwrap()
                .effects
                .as_slice(),
            &[EventEffect::AddRelic(
                RelicId::from_name("TEA_OF_DISCOURTESY").unwrap()
            )]
        );
    }

    #[test]
    fn unrest_site_heals_to_full_or_loses_max_hp_before_relic_reward() {
        let mut event = initialize(EventId::UnrestSite, 4100000, 99, 24, 63, 90).unwrap();
        assert_eq!(
            event.options[0].dynamic_description.unwrap().render(),
            "Heal 27 HP, gain Poor Sleep curse"
        );
        assert_eq!(
            choose(&mut event, "rest").unwrap().effects.as_slice(),
            &[
                EventEffect::Heal(27),
                EventEffect::AddCard(CardId::from_name("POOR_SLEEP").unwrap()),
            ]
        );
        assert_eq!(
            choose(&mut event, "kill").unwrap().effects.as_slice(),
            &[EventEffect::LoseMaxHp(8)]
        );
    }
}
