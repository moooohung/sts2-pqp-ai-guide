use super::{Character, MpRoster};
use crate::{combat::{RunCtx, RunRngs, ChoiceScheduling, REF_CACHE_WORDS}, identity::EncounterId, rng::Rng,
    tables::{RelicId, RelicRarity, RELIC_INFO, RoomType}};
use super::super::{catalog::*, map::{ActMap, generate_act_map}, rooms::{GeneratedRooms, generate_rooms_for_party}};

#[derive(Clone, Debug)]
pub struct MpWorld {
    pub run: RunCtx,
    pub(super) initial_run: RunCtx,
    /// RunRngSet.ToSerializable saves StringSeed, not the decoded numeric Seed.
    pub string_seed: String,
    /// RunState.ActFloor, assigned by EnterMapPointInternal(coord.row + 1).
    /// Unlike TotalFloor this resets in each new act.
    pub act_floor: i32,
    /// EventRoom.CanonicalEvent.Id / CombatRoom.Encounter.Id.
    pub current_room: Option<&'static str>,
    /// AbstractRoom.Enter allocates the current room's local ID. The supported
    /// base-room path starts at 0 after RunState.AddVisitedMapCoord resets it.
    pub room_id: Option<i32>,
    pub map: ActMap,
    /// RunState.AddVisitedMapCoord history; public map information only.
    pub visited: Vec<(i32, crate::run::Coord)>,
    pub player_bags: Vec<[Vec<RelicId>; 4]>,
    /// RelicGrabBag._mpFallbackDequeue, in original MoveToFallback order.
    pub player_fallbacks: Vec<Vec<RelicId>>,
    pub shared_bags: [Vec<RelicId>; 4],
    pub(crate) encounters: GeneratedRooms,
    pub(crate) act_defs: [&'static ActDef; 3],
    pub(crate) encounters_by_act: Vec<GeneratedRooms>,
    pub(crate) map_rngs: Vec<Rng>,
}

fn ids(names: &[&str]) -> Vec<RelicId> {
    names.iter().map(|name| RelicId::from_name(name).expect("audited relic identity")).collect()
}

impl MpWorld {
    /// CombatRoom.Encounter.Id, derived from the current room so it can never
    /// outlive the room: event, shop, rest and treasure rooms yield None.
    pub fn current_encounter(&self) -> Option<EncounterId> {
        self.current_room.and_then(EncounterId::from_name)
    }
    pub(crate) fn character_relic_names(character:Character) -> &'static [&'static str] {
        match character {
            Character::Ironclad => &["BRIMSTONE", "BURNING_BLOOD", "CHARONS_ASHES", "DEMON_TONGUE",
                "PAPER_PHROG", "RED_SKULL", "RUINED_HELMET", "SELF_FORMING_CLAY"],
            Character::Silent => &["HELICAL_DART", "NINJA_SCROLL", "PAPER_KRANE", "RING_OF_THE_SNAKE",
                "SNECKO_SKULL", "TINGSHA", "TOUGH_BANDAGES", "TWISTED_FUNNEL"],
            Character::Defect => &["CRACKED_CORE", "DATA_DISK", "EMOTION_CHIP", "GOLD_PLATED_CABLES",
                "POWER_CELL", "METRONOME", "RUNIC_CAPACITOR", "SYMBIOTIC_VIRUS"],
            Character::Necrobinder => &["BIG_HAT", "BONE_FLUTE", "BOOK_REPAIR_KNIFE", "BOOKMARK",
                "BOUND_PHYLACTERY", "FUNERARY_MASK", "IVORY_TILE", "UNDYING_SIGIL"],
            Character::Regent => &["DIVINE_RIGHT", "FENCING_MANUAL", "GALACTIC_DUST", "LUNAR_PASTRY",
                "MINI_REGENT", "ORANGE_DOUGH", "REGALITE", "VITRUVIAN_MINION"],
        }
    }
    /// Original seed string, not the Gym reset-seed adapter.
    pub fn new(seed: &str, roster: &mut MpRoster, ascension: i32) -> Result<Self, &'static str> {
        Self::initialize(seed, super::rng::decoded_run_seed(seed), roster, ascension)
    }

    pub fn from_numeric_seed(seed: u64, roster: &mut MpRoster, ascension: i32) -> Result<Self, &'static str> {
        Self::initialize(&seed.to_string(), seed, roster, ascension)
    }

    fn initialize(seed: &str, hash: u64, roster: &mut MpRoster, ascension: i32) -> Result<Self, &'static str> {
        let personal = &roster.players()[0].rngs;
        let rngs = RunRngs {
            seed: hash, up_front: Rng::named(hash, "up_front"), shuffle: Rng::named(hash, "shuffle"),
            unknown_map_point: Rng::named(hash, "unknown_map_point"),
            combat_card_generation: Rng::named(hash, "combat_card_generation"),
            combat_potion_generation: Rng::named(hash, "combat_potion_generation"),
            combat_card_selection: Rng::named(hash, "combat_card_selection"),
            combat_energy_costs: Rng::named(hash, "combat_energy_costs"),
            combat_targets: Rng::named(hash, "combat_targets"), monster_ai: Rng::named(hash, "monster_ai"),
            niche: Rng::named(hash, "niche"), combat_orbs: Rng::named(hash, "combat_orbs"),
            treasure_room: Rng::named(hash, "treasure_room_relics"),
            rewards: personal.rewards.clone(), shops: personal.shops.clone(), transformations: personal.transformations.clone(),
        };
        let p = &roster.players()[0];
        let mut run = RunCtx {
            choice_scheduling: ChoiceScheduling::Live, all_cards_unlocked: Some(true), rngs,
            ascension, act_index: 0, total_floor: 1, coord: None, room_type: RoomType::Event,
            gold: p.gold, extra_gold_reward: 0,
            max_hp: p.max_hp, hp: p.hp, max_energy: p.max_energy,
            max_potion_slots: p.max_potion_slots as i32, base_orb_slots: p.base_orb_slots as i32,
            deck: p.deck.clone(), relics: p.relics.clone(), potions: p.potions.clone(),
            paels_tooth_cards: Vec::new(), next_card_id: roster.next_card_id, ref_cache: [0; REF_CACHE_WORDS],
        };
        let mut shared_bags = [ids(SHARED_RELICS_COMMON), ids(SHARED_RELICS_UNCOMMON),
            ids(SHARED_RELICS_RARE), ids(SHARED_RELICS_SHOP)];
        for i in [1, 0, 2, 3] { run.rngs.up_front.shuffle(&mut shared_bags[i]); }
        run.rngs.up_front.shuffle(&mut ids(&["FRESNEL_LENS"]));
        run.rngs.up_front.shuffle(&mut ids(&["LOOMING_FRUIT", "VERY_HOT_COCOA"]));
        let mut player_bags = Vec::new();
        for p in roster.players() {
            // Character RelicPool.GenerateAllRelics source order, not a sort
            // of table IDs. Defect deliberately lists PowerCell before Metronome.
            let own_names=Self::character_relic_names(p.character);
            let mut bags = [ids(SHARED_RELICS_COMMON), ids(SHARED_RELICS_UNCOMMON),
                ids(SHARED_RELICS_RARE), ids(SHARED_RELICS_SHOP)];
            for (i, rarity) in [RelicRarity::Common, RelicRarity::Uncommon, RelicRarity::Rare, RelicRarity::Shop].into_iter().enumerate() {
                bags[i].extend(ids(own_names).into_iter()
                    .filter(|id| RELIC_INFO[id.index()].rarity == rarity));
            }
            for i in [1, 0, 2, 3] { run.rngs.up_front.shuffle(&mut bags[i]); }
            player_bags.push(bags);
        }
        roster.apply_starting_ascension(ascension)?;
        // Immutable initialization prefix, before act/map generation and Neow.
        let mut initial_run=run.clone();
        let initial_player=&roster.players()[0];
        initial_run.total_floor=0;
        initial_run.deck=initial_player.deck.clone();
        initial_run.max_potion_slots=initial_player.max_potion_slots as i32;
        let first = if Rng::named(hash, "act_selection").choice_index(2) == 0 { &ACT0 } else { &ACT1 };
        let mut ancients = SHARED_ANCIENTS.to_vec();
        run.rngs.up_front.shuffle(&mut ancients);
        let mut subsets = [Vec::new(), Vec::new(), Vec::new()];
        for subset in subsets.iter_mut().skip(1) {
            let count = run.rngs.up_front.next_int_exclusive(0, ancients.len() as i64 + 1) as usize;
            subset.extend(ancients.drain(..count));
        }
        let act_defs = [first, &ACT2, &ACT3];
        let mut generated = Vec::new();
        for (i, act) in act_defs.into_iter().enumerate() {
            generated.push(generate_rooms_for_party(act, &mut run.rngs.up_front, &subsets[i], ascension >= 10 && i == 2, true));
        }
        let mut map_rngs = (0..3)
            .map(|i| Rng::named(hash, &format!("act_{}_map", i + 1)))
            .collect::<Vec<_>>();
        let map = generate_act_map(first.num_rooms - 1, &mut map_rngs[0], ascension, 0);
        let start = map.points[map.start as usize].coord;
        run.coord = Some((start.col, start.row));
        for i in 0..roster.players().len() {
            let p = roster.player_mut(i).unwrap();
            p.hp = if ascension >= 2 { (p.max_hp as f64 * 0.8) as i32 } else { p.max_hp };
        }
        let encounters = generated[0].clone();
        Ok(Self { run, initial_run, string_seed: seed.to_owned(), act_floor: start.row + 1,
            current_room: Some("NEOW"), room_id: Some(0),
            map, visited: vec![(0, start)], player_bags, player_fallbacks: vec![Vec::new(); roster.players().len()],
            shared_bags, encounters, act_defs, encounters_by_act: generated, map_rngs })
    }

    /// RelicGrabBag.GetAvailableDeque/PullFromFront and RelicFactory's final
    /// Circlet fallback. Reward rarity rolls select Common, Uncommon or Rare.
    pub(super) fn pull_player_relic_from_front(&mut self, roster: &MpRoster,
        seat: usize, rarity: usize) -> RelicId {
        self.pull_player_relic_from_front_matching(roster,seat,rarity, |_|true)
    }

    pub(super) fn pull_player_relic_from_front_matching(&mut self, roster: &MpRoster,
        seat: usize, rarity: usize, predicate: impl Fn(&RelicId)->bool) -> RelicId {
        let floor = self.run.total_floor;
        let allowed = |id: &RelicId| super::late_treasure::relic_allowed(roster, floor, *id);
        // C# removes disallowed entries from every rarity and the fallback
        // queue before looking for the requested rarity or any higher one.
        for bag in &mut self.player_bags[seat] { bag.retain(allowed); }
        self.player_fallbacks[seat].retain(allowed);
        // GetAvailableDeque's additional predicate does not prune unmatched
        // relics: they remain available for later pulls without this predicate.
        let id = (rarity..3).find_map(|bucket| self.player_bags[seat][bucket].iter()
                .position(&predicate).map(|index|(bucket,index)))
            .map(|(bucket,index)| self.player_bags[seat][bucket].remove(index))
            .or_else(|| self.player_fallbacks[seat].iter().position(&predicate)
                .map(|index| self.player_fallbacks[seat].remove(index)))
            .unwrap_or(RelicId::Circlet);
        for bag in &mut self.shared_bags { bag.retain(|other| *other != id); }
        id
    }

    pub(super) fn move_player_relic_to_fallback(&mut self, seat: usize, id: RelicId) {
        // RelicGrabBag.MoveToFallback only queues a relic actually found in a
        // normal deque, removing all occurrences but appending it only once.
        let mut found = false;
        for bag in &mut self.player_bags[seat] {
            found |= bag.contains(&id);
            bag.retain(|other| *other != id);
        }
        if found { self.player_fallbacks[seat].push(id); }
    }
}
