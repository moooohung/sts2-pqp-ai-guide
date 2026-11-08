use crate::identity::EventId;
use super::{MpWorld, MpRoster, PlayerSpec, MapVoting, MapVote};
use crate::{card::Card, combat::Combat, relic::Relic,
    tables::{CardId, CardRarity, PotionId, RelicId, RoomType}, run::Coord};
#[path = "replay_view.rs"]
mod replay_view;
#[path = "determinize_mp.rs"]
mod determinize_mp;

fn model_id_entry(type_name: &str) -> String {
    let mut out = String::with_capacity(type_name.len() + 8);
    for (i, c) in type_name.chars().enumerate() {
        if i != 0 && c.is_ascii_uppercase() { out.push('_'); }
        out.push(c.to_ascii_uppercase());
    }
    out
}

#[cfg(test)]
#[path = "reward_guard_tests.rs"]
mod reward_guard_tests;

#[cfg(test)]
#[path = "progression_tests.rs"]
mod progression_tests;

#[cfg(test)]
#[path = "unverified_paths_tests.rs"]
mod unverified_paths_tests;

#[cfg(test)]
#[path = "layout_inventory_tests.rs"]
mod layout_inventory_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MpPhase { Neow, Map, Combat, Rewards, Event, Shop, RestSite, Treasure, RunOver }

/// Actor-bound MP driver. Unsupported rooms and pickups are explicit errors;
/// never construct one independent solo manager for each player.
#[derive(Clone, Debug)]
pub struct MpRun {
    queue_enabled: bool,
    queue_recording: bool,
    next_queue_combat: u64,
    pub queue_events: Vec<crate::action_queue::RunQueueEvent>,
    pub roster: MpRoster,
    pub world: MpWorld,
    pub phase: MpPhase,
    pub voting: MapVoting,
    pub neow: Vec<[RelicId; 3]>,
    pub neow_rngs: Vec<crate::rng::Rng>,
    pub neow_cards: Vec<Option<Vec<Card>>>,
    pub(super) neow_card_queues: Vec<Vec<Vec<Card>>>,
    pub(super) neow_card_sources: Vec<Option<RelicId>>,
    pub neow_potion_rewards: Vec<Option<PotionId>>,
    pub scroll_box_bundles: Vec<Option<Vec<Vec<Card>>>>,
    neow_done: Vec<bool>,
    pub(super) combat: Option<Combat>,
    normal_cursor: usize,
    elite_cursor: usize,
    boss_cursor: usize,
    pub rewards: Option<Vec<super::PlayerRewards>>,
    pub paels_growth_choices: Vec<Option<Vec<u32>>>,
    pub shears_choices: Vec<Option<Vec<u32>>>,
    pub new_leaf_choices: Vec<Option<Vec<u32>>>,
    pub pomander_choices: Vec<Option<Vec<u32>>>,
    pub precise_scissors_choices: Vec<Option<Vec<u32>>>,
    /// SmallCapsule's retained, recipient-owned RelicReward selection screen.
    pub relic_rewards: Vec<Option<RelicId>>,
    pub(super) relic_reward_skippable: Vec<bool>,
    pub(super) relic_reward_queues: Vec<Vec<(RelicId, bool)>>,
    pub(super) neows_bones_curse_pending: Vec<bool>,
    pub local_net_id: u64,
    pub seen_relics: std::collections::HashSet<RelicId>,
    /// SaveManager.Progress.DiscoveredPotions. This is profile-wide seen state,
    /// not the per-run Player.DiscoveredPotions list serialized on each seat.
    pub progress_discovered_potions: std::collections::HashSet<PotionId>,
    /// Each owner's supplied SaveManager.Progress.WongoPoints, not run-earned
    /// ExtraFields.WongoPoints. Unknown profile progress must remain explicit.
    pub saved_wongo_points: Vec<Option<i32>>,
    pub cheese_cards: Vec<Option<Vec<Card>>>,
    pub(super) event_card_modals: Vec<Option<super::events::EventCardModal>>,
    pub(super) event_card_reward_queues:Vec<Vec<super::PlayerRewards>>,
    pub(super) sphere_loot: Vec<Vec<Option<super::crystal_sphere::SphereLoot>>>,
    pub(super) event_potion_rewards: Vec<Option<[Option<crate::tables::PotionId>; 3]>>,
    pub(super) event_done: Vec<bool>,
    pub(super) event_cursor: usize,
    pub(super) events: Vec<Option<super::super::events::EventState>>,
    pub(super) native_event: Option<crate::identity::EventId>,
    pub(super) punch_off_owners: Vec<usize>,
    pub(super) shared_event_votes: Vec<Option<usize>>,
    pub(super) shared_event_selection_rng: crate::rng::Rng,
    pub(super) unknown_odds: [f32; 4],
    pub(super) shops: Vec<super::super::shop::ShopState>,
    pub(super) merchant_pickups: Vec<Option<super::merchant_pickups::MerchantPickup>>,
    pub(super) rest_sites: Vec<super::rest_site::SeatRest>,
    pub treasure_relics: Vec<RelicId>,
    pub(super) treasure_votes: Vec<Option<Option<usize>>>,
    pub(super) treasure_opened: Vec<bool>,
}

impl MpRun {
    /// Authenticated, atomic merchant command; implementation lives in merchant.rs.
    pub fn merchant_purchase(&mut self, net_id: u64, seat: usize, action: super::MerchantAction)
        -> Result<bool, &'static str> {
        let mut next = self.clone();
        let bought = next.purchase_merchant(net_id, seat, action)?;
        if bought { *self = next; }
        Ok(bought)
    }

    pub fn new(seed: &str, specs: &[PlayerSpec], ascension: i32) -> Result<Self, &'static str> {
        let mut roster = MpRoster::new(seed, specs)?;
        let world = MpWorld::new(seed, &mut roster, ascension)?;
        let (neow, neow_rngs) = roster.neow_events(seed).into_iter().unzip();
        let (col, row) = world.run.coord.unwrap();
        let voting = MapVoting::new(seed, specs.len(), 0, Coord::new(col, row))?;
        Ok(Self::initialize(specs, roster, world, neow, neow_rngs, voting))
    }

    /// Replay an already-derived, modern (non-`old`) game seed, not seed text.
    /// RunRngSet.cs constructor hashes text once; Player.cs:239 adds the seat
    /// to that hash. All named streams and derived offers/maps must be built
    /// from this value before any RNG is consumed, never restored afterward.
    pub fn from_numeric_seed(seed: u64, specs: &[PlayerSpec], ascension: i32) -> Result<Self, &'static str> {
        let mut roster = MpRoster::from_numeric_seed(seed, specs)?;
        let world = MpWorld::from_numeric_seed(seed, &mut roster, ascension)?;
        let (neow, neow_rngs) = roster.neow_events_from_numeric_seed(seed).into_iter().unzip();
        let (col, row) = world.run.coord.unwrap();
        let voting = MapVoting::from_numeric_seed(seed, specs.len(), 0, Coord::new(col, row))?;
        Ok(Self::initialize(specs, roster, world, neow, neow_rngs, voting))
    }

    fn initialize(specs: &[PlayerSpec], roster: MpRoster, world: MpWorld,
        neow: Vec<[RelicId; 3]>, neow_rngs: Vec<crate::rng::Rng>, mut voting: MapVoting) -> Self {
        let seen_relics = roster.players().iter().flat_map(|p| p.relics.iter().map(|r| r.id)).collect();
        let shared_event_selection_rng=crate::rng::Rng::named(world.run.rngs.seed,"event_synchronizer");
        voting.before_map_generated();
        Self { queue_enabled:true, queue_recording:false, next_queue_combat:0, queue_events:Vec::new(),
            roster, world, neow, neow_rngs, voting, phase: MpPhase::Neow,
            neow_cards: vec![None; specs.len()], neow_card_queues: vec![Vec::new(); specs.len()],
            neow_card_sources: vec![None; specs.len()], neow_potion_rewards: vec![None; specs.len()],
            scroll_box_bundles: vec![None; specs.len()],
            neow_done: vec![false; specs.len()],
            combat: None, normal_cursor: 0, elite_cursor: 0, boss_cursor: 0, rewards: None,
            paels_growth_choices: vec![None; specs.len()], shears_choices: vec![None; specs.len()],
            new_leaf_choices: vec![None; specs.len()],
            pomander_choices: vec![None; specs.len()], precise_scissors_choices: vec![None; specs.len()],
            relic_rewards: vec![None; specs.len()], relic_reward_skippable: vec![true; specs.len()],
            relic_reward_queues: vec![Vec::new(); specs.len()], neows_bones_curse_pending: vec![false; specs.len()],
            local_net_id: specs[0].net_id, seen_relics, progress_discovered_potions: std::collections::HashSet::new(),
            saved_wongo_points: vec![super::super::RunOptions::default().saved_wongo_points;specs.len()],
            cheese_cards: vec![None; specs.len()], event_card_modals: vec![None; specs.len()],
            event_card_reward_queues:vec![Vec::new();specs.len()],
            sphere_loot: vec![Vec::new(); specs.len()],
            event_potion_rewards: vec![None; specs.len()],
            event_done: vec![false; specs.len()],
            // C# RunManager.EnterRoom marks Neow's EventRoom visited even
            // though it came from an Ancient map point, advancing RoomSet.eventsVisited.
            event_cursor: 1, events: vec![None; specs.len()], native_event: None, punch_off_owners: Vec::new(),
            shared_event_votes: vec![None;specs.len()], shared_event_selection_rng,
            unknown_odds: [0.1, -1.0, 0.02, 0.03], shops: Vec::new(),
            merchant_pickups:vec![None;specs.len()],
            rest_sites: Vec::new(), treasure_relics: Vec::new(),
            treasure_votes: vec![None; specs.len()], treasure_opened: vec![false; specs.len()] }
    }

    pub fn seed_progress_discovered_potions<I>(&mut self, potions: I)
    where
        I: IntoIterator<Item = PotionId>,
    {
        self.progress_discovered_potions.clear();
        self.progress_discovered_potions.extend(potions);
    }

    /// ProgressSaveManager.MarkPotionAsSeen: the profile set is updated first;
    /// only a newly-seen potion owned by the local player is appended to that
    /// player's run-local DiscoveredPotions list.
    pub(super) fn mark_local_potion_seen(&mut self, seat: usize, potion: PotionId) {
        if self.roster.player(seat).unwrap().net_id == self.local_net_id
            && self.progress_discovered_potions.insert(potion)
        {
            self.roster.player_mut(seat).unwrap().discovered_potions.push((
                "POTION".to_owned(), model_id_entry(potion.name())));
        }
    }

    pub fn choose_neow(&mut self, net_id: u64, seat: usize, option: usize) -> Result<(), &'static str> {
        // A rejected nested pickup cannot leave half an acquired relic or an
        // advanced personal RNG behind. Neow executes once per seat.
        let mut next = self.clone();
        next.choose_neow_inner(net_id, seat, option)?;
        *self = next;
        Ok(())
    }

    fn choose_neow_inner(&mut self, net_id: u64, seat: usize, option: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow || option >= 3 || self.neow_done[seat] {
            return Err("invalid Neow action");
        }
        let id = self.neow[seat][option];
        self.obtain_neow_relic_effect(seat, id)?;
        self.neow_done[seat] = true;
        self.advance_neow();
        Ok(())
    }

    pub(super) fn advance_neow(&mut self) {
        if self.phase == MpPhase::Neow && self.neow_done.iter().all(|done| *done)
            && self.neow_cards.iter().all(Option::is_none)
            && self.neow_card_queues.iter().all(Vec::is_empty)
            && self.neow_potion_rewards.iter().all(Option::is_none)
            && self.scroll_box_bundles.iter().all(Option::is_none)
            && self.neows_bones_curse_pending.iter().all(|pending| !*pending)
            && (0..self.roster.players().len()).all(|seat| !self.owner_pickup_pending(seat)) {
            self.phase = MpPhase::Map;
        }
    }

    pub fn neow_finished(&self, seat: usize) -> Option<bool> {
        self.neow_done.get(seat).map(|done| *done && self.neow_cards[seat].is_none()
            && self.neow_card_queues[seat].is_empty()
            && self.neow_potion_rewards[seat].is_none()
            && self.scroll_box_bundles[seat].is_none()
            && !self.neows_bones_curse_pending[seat]
            && !self.owner_pickup_pending(seat))
    }

    pub(super) fn record_relic_seen(&mut self, seat: usize, id: RelicId) {
        // RelicCmd.Obtain calls MarkRelicAsSeen only for the local player.
        // ProgressSaveManager records a discovery only on the first profile
        // sighting. Starting relics were marked before their owner was attached.
        if self.roster.player(seat).unwrap().net_id == self.local_net_id && self.seen_relics.insert(id) {
            self.roster.player_mut(seat).unwrap().discovered_relics.push(("RELIC".to_owned(), id.name().to_owned()));
        }
    }

    pub fn select_neow_card(&mut self, net_id: u64, seat: usize, index: Option<usize>) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow { return Err("not at Neow card choice"); }
        let mut next = self.clone();
        next.finish_neow_card_choice(seat, index)?;
        *self = next;
        Ok(())
    }

    fn finish_neow_card_choice(&mut self, seat: usize, index: Option<usize>) -> Result<(), &'static str> {
        let cards = self.neow_cards[seat].as_ref().ok_or("no owner Neow card choice")?;
        if index.is_some_and(|i| i >= cards.len()) { return Err("invalid Neow card index"); }
        let cards = self.neow_cards[seat].take().unwrap();
        let source = self.neow_card_sources[seat].take();
        if let Some(i) = index {
            // CardReward.OnSelect uses CardPileCmd.Add, including owner-local
            // inventory modifiers. Keep the screen on any acquisition refusal.
            super::cards::add(&mut self.roster, &mut self.world.run, seat, cards[i].clone())?;
        }
        if source == Some(RelicId::HeftyTablet) {
            self.add_owner_card_from_id(seat, CardId::Injury)?;
        }
        if !self.neow_card_queues[seat].is_empty() {
            self.neow_cards[seat] = Some(self.neow_card_queues[seat].remove(0));
        }
        self.maybe_finish_neows_bones(seat)?;
        self.advance_neow();
        Ok(())
    }

    pub fn select_neow_potion_reward(&mut self, net_id: u64, seat: usize, accept: bool)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Neow { return Err("not at Neow potion reward"); }
        let potion = self.neow_potion_rewards[seat].ok_or("no owner Neow potion reward")?;
        let mut next = self.clone();
        if accept {
            let player = next.roster.player_mut(seat).unwrap();
            if !player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                let slot = player.potions.iter_mut().find(|p| p.is_none())
                    .ok_or("Neow potion reward inventory full")?;
                *slot = Some(potion);
                next.mark_local_potion_seen(seat, potion);
            }
        }
        next.neow_potion_rewards[seat] = None;
        next.maybe_finish_neows_bones(seat)?;
        next.advance_neow();
        *self = next;
        Ok(())
    }

    pub fn vote(&mut self, net_id: u64, seat: usize, destination: Coord) -> Result<(), &'static str> {
        let mut next = self.clone();
        next.vote_inner(net_id, seat, destination)?;
        *self = next;
        Ok(())
    }

    fn vote_inner(&mut self, net_id: u64, seat: usize, destination: Coord) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Map { return Err("not at map vote"); }
        if (0..self.roster.players().len()).any(|seat| self.owner_pickup_pending(seat)) {
            return Err("owner pickup choice pending");
        }
        let source = self.world.run.coord.map(|(col,row)|Coord::new(col,row))
            .unwrap_or(self.world.map.points[self.world.map.start as usize].coord);
        let point = self.world.map.point(source).ok_or("map source missing")?;
        let reachable=if self.world.run.coord.is_none(){destination==source}else{
            point.children.iter().any(|&i|self.world.map.points[i as usize].coord==destination)};
        if !reachable {
            return Err("map edge not reachable");
        }
        if let Some(coord) = self.voting.vote(seat, (self.world.run.act_index as usize, source), Some(MapVote {
            generation: self.voting.generation, coord: destination
        }))? {
            let kind = self.world.map.point(coord).ok_or("map destination missing")?.point_type;
            if !matches!(kind, crate::tables::MapPointType::Monster |
                crate::tables::MapPointType::Elite |
                crate::tables::MapPointType::Boss |
                crate::tables::MapPointType::Ancient |
                crate::tables::MapPointType::Shop | crate::tables::MapPointType::Unknown |
                crate::tables::MapPointType::RestSite | crate::tables::MapPointType::Treasure) {
                return Err("multiplayer room continuation not yet supported");
            }
            self.world.run.coord = Some((coord.col, coord.row));
            self.world.visited.push((self.world.run.act_index, coord));
            self.world.act_floor = coord.row + 1;
            self.world.run.total_floor += 1;
            self.world.room_id = Some(0);
            match kind {
                crate::tables::MapPointType::Ancient=>{
                    let id=self.world.encounters.ancient.ok_or("ancient event missing")?;
                    self.world.current_room=Some(id.name());self.world.run.room_type=RoomType::Event;
                    self.event_cursor+=1;self.phase=MpPhase::Event;
                    self.begin_generic_event(id)?;
                    self.apply_room_entry_hooks(RoomType::Event)?;
                }
                crate::tables::MapPointType::Monster => self.enter_monster()?,
                crate::tables::MapPointType::Elite => self.enter_elite()?,
                crate::tables::MapPointType::Boss => self.enter_boss()?,
                _ => return self.enter_noncombat(kind, coord),
            }
        }
        Ok(())
    }

    // RunManager.CreateRoom uses Act.PullNextEncounter(Monster) for both
    // fixed monster points and UnknownMapPointOdds monster outcomes.
    pub(super) fn enter_monster(&mut self) -> Result<(), &'static str> {
        let encounter = *self.world.encounters.normal.get(self.normal_cursor).ok_or("encounters exhausted")?;
        self.normal_cursor += 1;
        self.apply_room_entry_hooks(RoomType::Monster)?;
        self.combat = Some(self.roster.enter_combat_id(self.world.run.clone(), 0, encounter, RoomType::Monster)?);
        self.world.current_room = Some(encounter.name());
        self.world.room_id = Some(0);
        self.world.run.room_type = RoomType::Monster;
        self.phase = MpPhase::Combat;
        Ok(())
    }

    // ActModel.PullNextEncounter / RoomSet.NextEliteEncounter use the shared
    // elite visit cursor, independent of which player cast the winning map vote.
    pub(super) fn enter_elite(&mut self) -> Result<(), &'static str> {
        let encounter = *self.world.encounters.elite.get(self.elite_cursor).ok_or("elite encounters exhausted")?;
        self.elite_cursor += 1;
        self.apply_room_entry_hooks(RoomType::Elite)?;
        self.combat = Some(self.roster.enter_combat_id(self.world.run.clone(), 0, encounter, RoomType::Elite)?);
        self.world.current_room = Some(encounter.name());
        self.world.room_id = Some(0);
        self.world.run.room_type = RoomType::Elite;
        self.phase = MpPhase::Combat;
        Ok(())
    }

    // C# RunManager.CreateRoom creates a CombatRoom for RoomType.Boss, while
    // RoomSet.NextBossEncounter switches to SecondBoss after one boss visit.
    pub(super) fn enter_boss(&mut self) -> Result<(), &'static str> {
        let encounter = if self.boss_cursor != 0 && self.world.encounters.second_boss.is_some() {
            self.world.encounters.second_boss.ok_or("second boss encounter missing")?
        } else {
            self.world.encounters.boss
        };
        self.boss_cursor += 1;
        self.apply_room_entry_hooks(RoomType::Boss)?;
        self.combat = Some(self.roster.enter_combat_id(self.world.run.clone(), 0, encounter, RoomType::Boss)?);
        self.world.current_room = Some(encounter.name());
        self.world.room_id = Some(0);
        self.world.run.room_type = RoomType::Boss;
        self.phase = MpPhase::Combat;
        Ok(())
    }

    pub fn combat(&self) -> Option<&Combat> { self.combat.as_ref() }
    pub fn combat_mut(&mut self) -> Option<&mut Combat> { self.combat.as_mut() }
    pub fn record_replay_combat(&mut self, recorder: &crate::between::Recorder) {
        if let Some(combat) = &mut self.combat { recorder.record_combat(combat); }
    }
    pub fn enable_legacy_replay_adapter(&mut self) { self.queue_enabled=false; }
    pub fn enable_queue_recording(&mut self) {self.queue_recording=true;}
    pub fn synchronize_combat_queue(&mut self)->Result<bool,&'static str>{
        let mut changed=false;
        if self.phase==MpPhase::Combat && self.queue_enabled {
            if let Some(c)=&mut self.combat {
                let before=c.action_queue.events.len();
                c.register_queue_choices();
                changed=c.action_queue.events.len()!=before;
            }
        }
        self.flush_queue_events();
        Ok(changed)
    }
    fn flush_queue_events(&mut self) {
        let Some(c)=&mut self.combat else{return};
        if c.is_over {c.action_queue.combat_ended(c.round);}
        if !self.queue_recording {c.action_queue.events.clear();return;}
        if c.action_queue.events.is_empty() {return;}
        c.action_queue.run_sequence_offset = self.queue_events.len() as u64
            - c.action_queue.events[0].sequence;
        let combat=match c.action_queue.run_combat_id {
            Some(id)=>id,None=>{
                self.next_queue_combat+=1;
                c.action_queue.run_combat_id=Some(self.next_queue_combat);
                self.next_queue_combat
            }
        };
        for mut event in c.action_queue.events.drain(..) {
            event.sequence=self.queue_events.len() as u64;
            self.queue_events.push(crate::action_queue::RunQueueEvent{combat,event});
        }
    }

    /// Frozen r7 public-boss feature source. The policy receives only this
    /// public act-1 boss identity, never hidden room-generation state.
    pub fn public_act1_boss(&self) -> &'static str { self.world.encounters.boss.name() }

    /// The frozen original-DLL oracle's route policy. This remains a fallback
    /// because r7 has no admitted map observation.
    pub fn fixed_route_next_coord(&self) -> Result<Coord, &'static str> {
        fn rank(
            map: &super::super::map::ActMap,
            index: u8,
            memo: &mut std::collections::HashMap<u8, f64>,
        ) -> f64 {
            if let Some(&value) = memo.get(&index) { return value; }
            let point = &map.points[index as usize];
            let here = match point.point_type {
                crate::tables::MapPointType::RestSite => 35.0,
                crate::tables::MapPointType::Shop => 5.0,
                crate::tables::MapPointType::Unknown => -15.0,
                crate::tables::MapPointType::Elite => -100.0,
                _ => 0.0,
            };
            let tail = point.children.iter().map(|&child| rank(map, child, memo))
                .reduce(f64::max).unwrap_or(0.0);
            let score = here + 0.98 * tail;
            memo.insert(index, score);
            score
        }
        let (col, row) = self.world.run.coord.ok_or("map location missing")?;
        let point = self.world.map.point(Coord::new(col, row)).ok_or("map point missing")?;
        let mut memo = std::collections::HashMap::new();
        point.children.iter().copied().max_by(|a, b| {
            rank(&self.world.map, *a, &mut memo)
                .partial_cmp(&rank(&self.world.map, *b, &mut memo)).unwrap()
                .then_with(|| self.world.map.points[*b as usize].coord.col
                    .cmp(&self.world.map.points[*a as usize].coord.col))
        }).map(|i| self.world.map.points[i as usize].coord).ok_or("map has no child")
    }

    pub fn offer_rewards(&mut self) -> Result<(), &'static str> {
        if self.phase != MpPhase::Rewards { return Err("not at rewards"); }
        if self.rewards.is_some() { return Ok(()); }
        // Carryback and every owner's reward generation form one transaction.
        // A late refusal must retain combat, bags, inventory and all RNG state.
        let mut next = self.clone();
        next.flush_queue_events();
        next.generate_rewards()?;
        *self = next;
        Ok(())
    }

    fn generate_rewards(&mut self) -> Result<(), &'static str> {
        let punch_off_owners = self.punch_off_owners.clone();
        let pending = self.combat.as_ref().ok_or("combat missing")?;
        let dummy = super::super::manager::battleworn_dummy::ENCOUNTERS.iter()
            .position(|id| Some(*id) == self.world.current_encounter())
            .map(|tier| (tier, !pending.escaped_enemies.is_empty()));
        // LavaRock.TryModifyRewards is owner-bound and fires exactly once at
        // the act-1 boss. Capture owners before combat carryback; its two
        // RelicRewards are populated after the ordinary room rewards.
        let lava_rock_owners: Vec<usize> = if pending.run.act_index == 0
            && pending.run.room_type == RoomType::Boss
        {
            pending.run.relics.iter()
                .filter(|relic| relic.id == RelicId::LavaRock && relic.enabled && !relic.flag1)
                .map(|relic| relic.owner as usize)
                .collect()
        } else {
            Vec::new()
        };
        // RewardsSet.WithRewardsFromRoom appends each recipient's extras.
        let extras=if dummy.is_some_and(|(_, timed_out)| timed_out) {
            Vec::new()
        } else { pending.end_rewards.clone() };
        if extras.iter().any(|reward|match reward {
            crate::combat::EndReward::Gold{owner,..}|crate::combat::EndReward::Card{owner,..}
            |crate::combat::EndReward::SpecialCard{owner,..}|crate::combat::EndReward::CardRemoval{owner}
                => *owner as usize>=pending.players.len(),
        }) {return Err("invalid multiplayer reward owner");}
        let legacy_gold:Vec<_>=(0..pending.players.len()).map(|seat|
            if dummy.is_some_and(|(_, timed_out)| timed_out) {0}
            else if seat==0 {pending.run.extra_gold_reward}else{pending.players[seat as u8].extra_gold_reward}).collect();
        let mut special_cards=vec![Vec::new();pending.players.len()];
        for reward in &pending.end_rewards {
            if dummy.is_some_and(|(_, timed_out)| timed_out) { break; }
            if let crate::combat::EndReward::SpecialCard{owner,card}=reward {
                let cards=special_cards.get_mut(*owner as usize)
                    .ok_or("invalid multiplayer special-card reward owner")?;
                cards.push(Some(card.clone()));
                // A unique priority-4 reward has an exact sorted index. Ties
                // need a separate original-DLL ordering proof, not a guessed
                // stable ordering for List<Reward>.Sort.
                if cards.len()>1 {return Err("unported multiplayer multiple special-card reward ordering");}
            }
        }
        let c = self.combat.take().ok_or("combat missing")?;
        let proportion = c.gold_proportion();
        self.world.run = self.roster.leave_combat(c)?;
        // RewardsSet.WithRewardsFromRoom returns before ordinary rewards AND
        // extras for the final act's boss. EnterNextAct then enters TheArchitect.
        if self.world.run.room_type==RoomType::Boss
            && self.world.run.act_index as usize+1>=self.world.act_defs.len() {
            self.rewards=Some(vec![super::PlayerRewards::default();self.roster.players().len()]);
            return Ok(());
        }
        let mut next_id = self.roster.next_card_id;
        let mut rewards = Vec::new();
        for seat in 0..self.roster.players().len() {
            if let Some((tier, timed_out)) = dummy {
                rewards.push(self.dummy_rewards(seat, tier, timed_out)?);
                next_id = self.roster.next_card_id;
            } else {
            rewards.push(super::rewards::generate(self.roster.player_mut(seat).unwrap(), seat as u8,
                self.world.run.ascension, self.world.run.act_index, &mut next_id, proportion,
                self.world.run.room_type,&mut self.world.run.rngs.niche)?);
            }
            rewards[seat].special_cards=std::mem::take(&mut special_cards[seat]);
            // GenerateRewardsFor builds the ordinary elite RelicReward before
            // WithRewardsFromRoom appends extras; Populate preserves this order.
            if self.world.run.room_type == RoomType::Elite {
                let relic = self.pull_event_relic_from_front(seat)?;
                rewards[seat].extra_relics.push(Some(relic));
            }
            let mut has_gold=false;
            for extra in &extras {
                match extra {
                    crate::combat::EndReward::Gold{owner,amount} if *owner as usize==seat => {
                        // GoldReward.Populate consumes a draw even for min == max.
                        self.roster.player_mut(seat).unwrap().rngs.rewards.next_int(*amount as i64,*amount as i64);
                        rewards[seat].gold.push(*amount);
                        has_gold=true;
                    }
                    crate::combat::EndReward::Card{owner,room} if *owner as usize==seat => {
                        let cards=super::rewards::generate_card_reward(self.roster.player_mut(seat).unwrap(),
                            seat as u8,self.world.run.ascension,self.world.run.act_index,&mut next_id,3,*room,&mut self.world.run.rngs.niche)?;
                        rewards[seat].extra_cards.push(Some(cards));
                    }
                    crate::combat::EndReward::CardRemoval{owner} if *owner as usize==seat => {
                        rewards[seat].card_removals.push(false);
                    }
                    _=>{}
                }
            }
            // The scalar is a legacy mirror of retained Gold entries, not a second reward.
            if !has_gold && legacy_gold[seat]!=0 {
                let amount=legacy_gold[seat] as i64;
                self.roster.player_mut(seat).unwrap().rngs.rewards.next_int(amount,amount);
                rewards[seat].gold.push(legacy_gold[seat]);
            }
        }
        // WongosMysteryTicket.TryModifyRewards runs after AfterCombatEnd has
        // incremented CombatsFinished. Its three owner-local rewards are
        // populated independently, then AfterModifyingRewards disables it.
        for owner in 0..self.roster.players().len() {
            let tickets:Vec<_>=self.roster.player(owner).unwrap().relics.iter().enumerate()
                .filter_map(|(i,r)|(r.id==RelicId::WongosMysteryTicket&&r.counter>=5&&!r.flag1).then_some(i))
                .collect();
            for index in tickets {
                for _ in 0..3 {
                    let relic=self.pull_event_relic_from_front(owner)?;
                    rewards[owner].extra_relics.push(Some(relic));
                }
                let ticket=&mut self.roster.player_mut(owner).unwrap().relics[index];
                ticket.flag1=true;
                ticket.enabled=false;
            }
        }
        for owner in lava_rock_owners {
            for _ in 0..2 {
                let relic = self.pull_event_relic_from_front(owner)?;
                rewards[owner].extra_relics.push(Some(relic));
            }
            let lava = self.roster.player_mut(owner).unwrap().relics.iter_mut()
                .find(|relic| relic.id == RelicId::LavaRock && relic.enabled && !relic.flag1)
                .ok_or("Lava Rock owner relic missing after combat")?;
            lava.flag1 = true;
            lava.enabled = false;
        }
        for owner in punch_off_owners {
            // PunchOff.Fight appends [RelicReward(owner), PotionReward(owner)]
            // after the ordinary Monster room rewards.
            let relic = self.pull_event_relic_from_front(owner)?;
            let potion = super::rewards::generate_potions(
                self.roster.player_mut(owner).unwrap(), 1)?
                .into_iter().next().ok_or("Punch Off potion reward missing")?;
            rewards[owner].extra_relics.push(Some(relic));
            rewards[owner].extra_potions.push(Some(potion));
        }
        self.punch_off_owners.clear();
        self.roster.next_card_id = next_id;
        self.world.run.extra_gold_reward=0;
        self.rewards = Some(rewards);
        Ok(())
    }

    pub fn take_extra_relic_reward(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Rewards { return Err("not at rewards"); }
        let id = self.rewards.as_ref().ok_or("rewards not offered")?
            .get(seat).ok_or("invalid reward owner")?
            .extra_relics.get(index).copied().flatten()
            .ok_or("extra relic reward unavailable")?;
        let mut next = self.clone();
        next.roster.obtain_inventory_relic(&mut next.world.run, seat, id)?;
        next.record_relic_seen(seat, id);
        next.rewards.as_mut().unwrap()[seat].extra_relics[index] = None;
        *self = next;
        Ok(())
    }

    /// SpecialCardReward.OnSelect adds the retained instance; Populate never
    /// generates a replacement or consumes RNG. Acquisition is transactional.
    pub fn take_special_card_reward(&mut self, net_id:u64, seat:usize, reward_index:usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id,seat)?;
        if self.phase!=MpPhase::Rewards {return Err("not at rewards");}
        let reward=self.rewards.as_ref().and_then(|rows|rows.get(seat))
            .ok_or("rewards not offered")?;
        let index=(0..reward.special_cards.len()).find(|&index|
            reward.special_card_reward_index(index)==Some(reward_index))
            .ok_or("special-card reward unavailable")?;
        let card=reward.special_cards[index].clone().unwrap();
        let mut next=self.clone();
        super::cards::add(&mut next.roster,&mut next.world.run,seat,card)?;
        next.rewards.as_mut().unwrap()[seat].special_cards[index]=None;
        *self=next;
        Ok(())
    }

    pub fn take_extra_potion_reward(&mut self, net_id: u64, seat: usize, index: usize)
        -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Rewards { return Err("not at rewards"); }
        let potion = self.rewards.as_ref().ok_or("rewards not offered")?
            .get(seat).ok_or("invalid reward owner")?
            .extra_potions.get(index).copied().flatten()
            .ok_or("extra potion reward unavailable")?;
        let mut next = self.clone();
        let procured = {
            let player = next.roster.player_mut(seat).unwrap();
            if player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                false
            } else if let Some(slot) = player.potions.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(potion);
                true
            } else {
                return Ok(false);
            }
        };
        if procured { next.mark_local_potion_seen(seat, potion); }
        next.rewards.as_mut().unwrap()[seat].extra_potions[index] = None;
        *self = next;
        Ok(true)
    }

    /// PotionReward.OnSelect: false only when inventory is full, leaving the
    /// offer available. Sozu denies procurement but still consumes the reward.
    /// PotionCmd.TryToProcure / Player.AddPotionInternal choose the first free
    /// slot of the recipient, never another seat's inventory.
    pub fn take_potion_reward(&mut self, net_id: u64, seat: usize) -> Result<bool, &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Rewards { return Err("not at rewards"); }
        let reward = self.rewards.as_mut().ok_or("rewards not offered")?
            .get_mut(seat).ok_or("invalid reward owner")?;
        if reward.completed || reward.potion_taken { return Err("potion reward already completed"); }
        let potion = reward.potion.ok_or("no potion reward")?;
        let mut procured = false;
        {
            let player = self.roster.player_mut(seat).unwrap();
            if !player.relics.iter().any(|r| r.id == RelicId::Sozu && r.enabled) {
                let Some(slot) = player.potions.iter_mut().find(|slot| slot.is_none()) else { return Ok(false); };
                *slot = Some(potion);
                procured = true;
            }
        }
        // BeltBuckle.AfterPotionProcured only refreshes presentation outside
        // combat; its Dexterity callback is gated on IsInProgress.
        reward.potion_taken = true;
        if procured { self.mark_local_potion_seen(seat, potion); }
        Ok(true)
    }

    /// PotionCmd.Discard: combat owns the live inventory until carryback;
    /// outside combat an owner can free a slot for a retained PotionReward.
    fn event_potion_inventory_locked(&self,seat:usize)->bool {
        // Both original events set Owner.CanUseOrRemovePotions=false until
        // their owner-local OnEventFinished callback, not the shared barrier.
        !self.native_event_finished(seat) && self.events[seat].as_ref()
            .is_some_and(|event|event.id==EventId::StoneOfAllTime || event.id==EventId::TheFutureOfPotions)
    }
    pub fn can_discard_potion(&self,net_id:u64,seat:usize,slot:usize)->bool{
        if self.roster.actor(net_id,seat).is_err(){return false;}
        if self.phase==MpPhase::Combat{
            return self.combat.as_ref().is_some_and(|c|c.can_discard_potion_for_player(seat as u8,slot));
        }
        matches!(self.phase,MpPhase::Neow|MpPhase::Map|MpPhase::Rewards|MpPhase::Event)
            &&self.combat.is_none()
            &&!self.event_potion_inventory_locked(seat)
            &&self.roster.player(seat).unwrap().potions.get(slot).is_some_and(Option::is_some)
    }

    pub fn discard_potion(&mut self, net_id: u64, seat: usize, slot: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase==MpPhase::Combat{
            self.combat.as_mut().ok_or("combat missing")?.discard_potion_for_player(seat as u8,slot)?;
            return Ok(());
        }
        if !matches!(self.phase, MpPhase::Neow | MpPhase::Map | MpPhase::Rewards | MpPhase::Event) {
            return Err("not at run potion inventory");
        }
        // EventModel's CombatStateForLayout is presentation-only. Potion
        // inventory still belongs to the run until Fight starts combat.
        if self.combat.is_some() && self.phase != MpPhase::Event { return Err("combat carryback pending"); }
        if self.event_potion_inventory_locked(seat) {
            return Err("event locks owner potion inventory");
        }
        let potion = self.roster.player_mut(seat).unwrap().potions.get_mut(slot)
            .ok_or("invalid owner potion slot")?;
        if potion.is_none() { return Err("empty owner potion slot"); }
        *potion = None;
        // BeltBuckle is the only AfterPotionDiscarded relic callback in the
        // audited DLL. Outside an active combat it only refreshes presentation.
        Ok(())
    }

    /// CardReward.OnSelect -> CardPileCmd.Add, using the recipient's retained
    /// offer. Acceptance does not reroll offers or advance any player's RNG.
    pub fn take_card_reward(&mut self, net_id: u64, seat: usize, index: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        let reward = self.active_card_reward(seat)?;
        if reward.completed || reward.card_taken { return Err("card reward already completed"); }
        let card = reward.cards.get(index).cloned().ok_or("invalid reward card index")?;
        // Acquisition hooks may fail. Retain the offer and all inventories/RNGs
        // unchanged on refusal, including shared template allocation state.
        let mut next = self.clone();
        super::cards::add(&mut next.roster, &mut next.world.run, seat, card)?;
        next.active_card_reward_mut(seat)?.card_taken = true;
        *self = next;
        Ok(())
    }

    pub fn take_extra_card_reward(&mut self, net_id:u64, seat:usize, reward:usize, index:usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id,seat)?;
        if self.phase!=MpPhase::Rewards {return Err("not at rewards");}
        let card=self.rewards.as_ref().and_then(|rows|rows.get(seat))
            .and_then(|row|row.extra_cards.get(reward)).and_then(Option::as_ref)
            .and_then(|cards|cards.get(index)).cloned().ok_or("extra card reward unavailable")?;
        let mut next=self.clone();
        super::cards::add(&mut next.roster,&mut next.world.run,seat,card)?;
        next.rewards.as_mut().unwrap()[seat].extra_cards[reward]=None;
        *self=next;
        Ok(())
    }

    pub fn take_gold_skip_remaining_rewards(&mut self, net_id: u64, seat: usize)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        let reward = self.rewards.as_mut().ok_or("rewards not offered")?.get_mut(seat).ok_or("invalid reward owner")?;
        if reward.completed { return Err("rewards already completed"); }
        if !reward.gold_taken {
            // Each GoldReward invokes GainGold independently (rounding and
            // AfterGoldGained), not once for the sum of all reward entries.
            for &amount in &reward.gold{
                self.roster.player_mut(seat).unwrap().gain_gold(&self.world.run,amount);
            }
            reward.gold_taken = true;
        }
        reward.completed = true;
        if self.rewards.as_ref().unwrap().iter().all(|r| r.completed) {
            self.phase = MpPhase::Map;
            if let Some((col,row))=self.world.run.coord {
                self.voting.on_location_changed(self.world.run.act_index as usize,Coord::new(col,row));
            }
            self.rewards = None;
        }
        Ok(())
    }

    /// PlayerCmd.EndTurn marks one seat ready; every living participant must
    /// reach the shared barrier before combat can progress.
    pub fn end_turn_for(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        let c=self.combat.as_mut().ok_or("combat missing")?;
        if self.queue_enabled {c.enqueue_player_action(seat as u8, crate::action_queue::CombatAction::EndTurn)?;}
        else {c.end_player_turn_for(seat as u8);}
        self.update_combat_phase()
    }

    pub fn unend_turn_for(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        self.combat.as_mut().ok_or("combat missing")?.enqueue_player_action(seat as u8, crate::action_queue::CombatAction::UnendTurn)?;
        self.update_combat_phase()
    }

    /// ReadyToBeginEnemyTurnAction is a second all-seat barrier after
    /// BeforeFlush. This remains actor-bound for whole-run policy consumers.
    pub fn ready_to_begin_enemy_turn_for(&mut self, net_id: u64, seat: usize) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        let c=self.combat.as_mut().ok_or("combat missing")?;
        if self.queue_enabled {c.enqueue_player_action(seat as u8, crate::action_queue::CombatAction::ReadyForEnemyTurn)?;}
        else {c.set_ready_to_begin_enemy_turn(seat as u8);}
        self.update_combat_phase()
    }

    /// Boss rewards close before the next shared act is generated. Keeping the
    /// transition explicit preserves the existing act-terminal replay boundary.
    pub fn advance_act(&mut self) -> Result<(), &'static str> {
        if self.phase != MpPhase::Map { return Err("not at act transition"); }
        let coordinate_free=self.world.run.coord.is_none();
        if let Some((col,row))=self.world.run.coord {
            let point=self.world.map.point(Coord::new(col,row)).ok_or("act transition point missing")?;
            if point.point_type!=crate::tables::MapPointType::Boss||!point.children.is_empty(){
                return Err("not at terminal boss");
            }
        } else if self.world.run.room_type!=RoomType::Boss||self.rewards.is_some() {
            return Err("not after debug boss rewards");
        }
        let current = self.world.run.act_index as usize;
        if current + 1 >= self.world.act_defs.len() {
            self.enter_architect();
            return Ok(());
        }
        let next = current + 1;
        self.set_act_internal(next)?;
        if coordinate_free {
            self.world.run.coord=None;
            self.world.room_id=Some(0); // EnterAct's MapRoom, AbstractRoom.Enter.
        }
        Ok(())
    }

    fn set_act_internal(&mut self,next:usize)->Result<(),&'static str> {
        if next>=self.world.act_defs.len() {return Err("debug act does not exist");}
        let def = self.world.act_defs[next];
        self.world.map = crate::run::map::generate_act_map(
            def.num_rooms - 1,
            &mut self.world.map_rngs[next],
            self.world.run.ascension,
            next,
        );
        let quests:Vec<_>=self.roster.players().iter().flat_map(|p|p.deck.iter())
            .filter(|c|c.id==CardId::SpoilsMap).map(|c|c.instance_id).collect();
        if next==1&&!quests.is_empty() {
            self.world.map=crate::run::map::generate_spoils_act_map(def.num_rooms-1,
                self.world.run.rngs.seed,self.world.run.ascension,next);
            if let Some(treasure)=self.world.map.points.iter_mut()
                .find(|p|p.point_type==crate::tables::MapPointType::Treasure) {
                treasure.spoils_quests.extend(quests);
            }
        }
        self.world.encounters = self.world.encounters_by_act[next].clone();
        let start = self.world.map.points[self.world.map.start as usize].coord;
        self.world.run.act_index = next as i32;
        self.world.run.coord = Some((start.col, start.row));
        self.world.visited.push((next as i32, start));
        self.world.run.room_type = RoomType::Event;
        self.world.act_floor = 0;
        self.world.current_room = None;
        self.world.room_id = None;
        self.normal_cursor = 0;
        self.elite_cursor = 0;
        self.boss_cursor = 0;
        self.event_cursor = 0;
        self.unknown_odds = [0.1, -1.0, 0.02, 0.03];
        self.voting.before_map_generated();
        self.voting.on_location_changed(next, start);
        Ok(())
    }

    /// C# Core/DevConsole/ConsoleCommands/ActConsoleCmd.cs integer jump then
    /// RoomConsoleCmd.cs "room boss"; both are networked commands.
    /// RunManager.SetActInternal clears visited coordinates and resets ActFloor;
    /// EnterRoomDebug appends history, not a map move. No HP/deck/relic grants.
    /// This intentionally constructs a debug boss trial, not a natural full run.
    pub fn new_debug_late_boss(seed:&str,specs:&[PlayerSpec],ascension:i32,act:usize)
        ->Result<Self,&'static str> {
        let mut run=Self::new_debug_act(seed,specs,ascension,act)?;
        run.world.run.total_floor+=1;
        let room_id=run.world.room_id.unwrap()+1;
        run.enter_boss()?;
        run.world.room_id=Some(room_id);
        Ok(run)
    }

    /// ActConsoleCmd integer jump, leaving the starting Ancient unvisited.
    pub fn new_debug_act(seed:&str,specs:&[PlayerSpec],ascension:i32,act:usize)
        ->Result<Self,&'static str> {
        if !(1..=2).contains(&act){return Err("debug coverage requires Act 2 or 3");}
        let mut run=Self::new(seed,specs,ascension)?;
        run.set_act_internal(act)?;run.world.run.coord=None;run.phase=MpPhase::Map;
        run.world.room_id=Some(0); // RunManager.EnterAct -> MapRoom.Enter -> id 0.
        Ok(run)
    }

    /// C# WinConsoleCmd.cs, not natural combat-policy coverage.
    pub fn debug_win_combat(&mut self)->Result<(),&'static str>{
        if self.phase!=MpPhase::Combat{return Err("debug win requires combat");}
        self.combat.as_mut().ok_or("combat missing")?.debug_win_combat()?;
        self.update_combat_phase()
    }

    /// RunManager.EnterNextAct after rewards from a debug-entered boss room.
    /// Coordinate-free debug room history is not a fabricated terminal map point.
    pub fn proceed_after_debug_boss_rewards(&mut self)->Result<(),&'static str>{
        if self.phase!=MpPhase::Map||self.world.run.room_type!=RoomType::Boss
            ||self.world.run.coord.is_some()||self.rewards.is_some() {
            return Err("not after debug boss rewards");
        }
        self.advance_act()
    }

    pub fn play_card(&mut self, net_id: u64, seat: usize, hand: usize, target: Option<usize>)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        let c = self.combat.as_mut().ok_or("combat missing")?;
        if !self.queue_enabled {
            if !c.play_card(seat as u8,hand,target) {return Err("combat action refused");}
            return self.update_combat_phase();
        }
        let card = *c.players[seat as u8].hand.get(hand).ok_or("combat card missing")?;
        let target = match c.card_target_type(card) {
            crate::tables::TargetType::AnyEnemy => target.and_then(|i| c.enemies.get(i).copied()),
            crate::tables::TargetType::AnyAlly => target.and_then(|i| c.player_side().get(i).copied()),
            _ => None,
        };
        c.enqueue_player_action(seat as u8, crate::action_queue::CombatAction::PlayCard { card, target })?;
        self.update_combat_phase()
    }

    pub fn use_potion(&mut self, net_id: u64, seat: usize, slot: usize, target: Option<usize>)
        -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        let c = self.combat.as_mut().ok_or("combat missing")?;
        if !self.queue_enabled {
            if !c.use_potion_for_player(seat as u8,slot,target) {return Err("combat potion refused");}
            return self.update_combat_phase();
        }
        let potion = c.potion_slots_for_player(seat as u8).and_then(|slots| slots.get(slot))
            .copied().flatten().ok_or("combat potion missing")?;
        let target = match crate::tables::POTION_INFO[potion.index()].target {
            crate::tables::PotionTargetType::AnyEnemy => target.and_then(|i| c.enemies.get(i).copied()),
            crate::tables::PotionTargetType::AnyPlayer => Some(target.unwrap_or(seat) as u8),
            _ => None,
        };
        c.enqueue_player_action(seat as u8, crate::action_queue::CombatAction::UsePotion { slot, target })?;
        self.update_combat_phase()
    }

    pub fn end_turn(&mut self) -> Result<(), &'static str> {
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        self.combat.as_mut().ok_or("combat missing")?.end_player_turn();
        self.update_combat_phase()
    }

    pub fn resolve_combat_choice(&mut self, net_id: u64, seat: usize, index: Option<usize>) -> Result<(), &'static str> {
        self.roster.actor(net_id, seat)?;
        if self.phase != MpPhase::Combat { return Err("not in combat"); }
        let c=self.combat.as_mut().ok_or("combat missing")?;
        if self.queue_enabled {c.resume_queued_choice(seat as u8,index)?;}
        else if !c.resolve_pending_choice_for_player(seat as u8,index) {return Err("combat choice refused");}
        self.update_combat_phase()
    }

    fn update_combat_phase(&mut self) -> Result<(), &'static str> {
        self.flush_queue_events();
        let c = self.combat.as_ref().ok_or("combat missing")?;
        if let Some(fault) = c.fault { return Err(fault); }
        if c.is_over { self.phase = if c.won { MpPhase::Rewards } else { MpPhase::RunOver }; }
        Ok(())
    }
}
