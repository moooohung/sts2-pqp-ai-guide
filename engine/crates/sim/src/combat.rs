//! r7 `core/combat.py` CombatState with `core/damage.py` and `core/hooks.py`, CPU-first:
//! arenas + small handles, closed-enum hook dispatch, no allocation per step once warm.
//!
//! Fail-closed rule: any r7 behaviour not ported yet sets `fault` and the caller stops the game
//! (r8 quarantines that lane) instead of silently diverging from r7.
use crate::character::Character;

use crate::card::{CId, Card, KRef};
use crate::creature::{Creature, PRef, Power, Side};
use crate::relic::Relic;
use crate::rng::Rng;
use crate::tables::*;
use arrayvec::ArrayVec;

pub const MAX_HAND_SIZE: usize = 10;
// Original OstyCmd.Summon, Byrdpip.AfterRoomEntered and PaelsLegion.AfterRoomEntered
// each register a distinct pet per player. LegionOfBone can summon Osty for ALL
// four players even when their two relic pets already occupy the ally roster.
// PlayerCombatState.cs:212-223 has no numeric pet-slot cap; this arena bound
// covers the three concrete reusable pet types, not a four-pet party limit.
pub const MAX_PET_ALLIES: usize = 3 * crate::player::MAX_PLAYERS;
pub const MAX_COMBAT_CREATURES: usize = crate::player::MAX_PLAYERS + MAX_PET_ALLIES + 8;
pub const BASE_DRAW: i32 = 5;
pub const BASE_ENERGY: i32 = 3;
pub const PLAYER: CId = 0;

/// `ValueProp` bit set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Props(pub u8);
impl Props {
    pub const NONE: Props = Props(0);
    pub const UNBLOCKABLE: Props = Props(2);
    pub const UNPOWERED: Props = Props(4);
    pub const MOVE: Props = Props(8);
    #[inline] pub const fn has(self, p: Props) -> bool { self.0 & p.0 != 0 }
    #[inline] pub const fn is_card_or_monster_move(self) -> bool { self.has(Props::MOVE) }
    #[inline] pub const fn is_powered_attack(self) -> bool { self.has(Props::MOVE) && !self.has(Props::UNPOWERED) }
    #[inline] pub const fn is_powered_block(self) -> bool { self.is_powered_attack() }
    #[inline] pub const fn or(self, p: Props) -> Props { Props(self.0 | p.0) }
}

/// Named run RNG streams (r7 `RunRngSet`), owned by the run and borrowed by combat.
#[derive(Clone, Debug)]
pub struct RunRngs {
    pub seed: u64,
    pub up_front: Rng,
    pub shuffle: Rng,
    pub unknown_map_point: Rng,
    pub combat_card_generation: Rng,
    pub combat_potion_generation: Rng,
    pub combat_card_selection: Rng,
    pub combat_energy_costs: Rng,
    pub combat_targets: Rng,
    pub monster_ai: Rng,
    pub niche: Rng,
    pub combat_orbs: Rng,
    pub treasure_room: Rng,
    pub rewards: Rng,
    pub shops: Rng,
    pub transformations: Rng,
}

/// Run-level state a combat reads or writes (r7 PlayerState/RunState pieces).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChoiceScheduling {
    /// Installed C# test selector: no SignalPlayerChoiceBegun before enumeration.
    Sequential,
    /// Live UI: signal, let other setup/side-start/orb tasks run, then enumerate.
    #[default]
    Live,
    /// Oracle adaptation: FromHand/FromChooseACardScreen signal, but the
    /// installed FromCombatPile test selector answers synchronously.
    LiveHandAndChooseOnly,
}

impl ChoiceScheduling {
    pub(crate) fn is_live(self) -> bool { self != Self::Sequential }

    pub(crate) fn signals_choice(self, source: PileKind) -> bool {
        self != Self::LiveHandAndChooseOnly
            || !matches!(source, PileKind::Draw | PileKind::Discard | PileKind::Exhaust | PileKind::Play)
    }
}

#[derive(Clone, Debug)]
pub struct RunCtx {
    pub choice_scheduling: ChoiceScheduling,
    /// Explicit run unlock configuration, preserved through combat ownership transfers.
    /// None means the input recording did not specify unlocks; partial unlocks fail closed.
    pub all_cards_unlocked: Option<bool>,
    pub rngs: RunRngs,
    pub ascension: i32,
    pub act_index: i32,
    pub total_floor: i32,
    pub coord: Option<(i32, i32)>,
    pub room_type: RoomType,
    pub gold: i32,
    /// Solo-player projection of CombatRoom.extra_rewards GoldReward entries created in combat.
    /// The run layer consumes this after combat; it is deliberately not credited to `gold` early.
    pub extra_gold_reward: i32,
    pub max_hp: i32,
    pub hp: i32,
    pub max_energy: i32,
    pub max_potion_slots: i32,
    /// `Player.BaseOrbSlotCount` (the character's: Defect 3, others 0); every combat opens this
    /// many orb slots (`PlayerCombatState` setup: `OrbQueue.AddCapacity`).
    pub base_orb_slots: i32,
    pub deck: Vec<Card>,
    // relic fire: relics/shop_event.py:2683
    pub paels_tooth_cards: Vec<Card>,
    pub relics: Vec<Relic>,
    pub potions: Vec<Option<PotionId>>,
    /// r7 global `cards.base._next_instance_id`.
    pub next_card_id: u32,
    /// r7 `lru_cache` of `_reference_card_base_values(card_id, upgraded)`: a miss builds a card
    /// and consumes an instance id. Bit `2 * card index + upgraded`.
    pub ref_cache: [u64; REF_CACHE_WORDS],
}

pub const REF_CACHE_WORDS: usize = (CardId::COUNT * 2).div_ceil(64);

impl RunCtx {
    /// C# Player.Relics contains only the queried player's inventory.
    /// Filtering ownership does not filter status: callers decide whether
    /// disabled/used-up models still qualify for their particular hook.
    pub fn owned_relics(&self, owner: CId) -> impl Iterator<Item = &Relic> {
        self.relics.iter().filter(move |r| r.owner == owner)
    }

    pub fn owned_relics_mut(&mut self, owner: CId) -> impl Iterator<Item = &mut Relic> {
        self.relics.iter_mut().filter(move |r| r.owner == owner)
    }

    pub fn has_owned_relic(&self, owner: CId, id: RelicId) -> bool {
        self.owned_relics(owner).any(|r| r.id == id)
    }

    /// Returns true on a cache miss (and records the key), like the r7 lru_cache.
    #[inline]
    pub fn ref_cache_miss(&mut self, id: CardId, upgraded: bool) -> bool {
        let bit = id.index() * 2 + upgraded as usize;
        let (w, m) = (bit / 64, 1u64 << (bit % 64));
        let miss = self.ref_cache[w] & m == 0;
        self.ref_cache[w] |= m;
        miss
    }

    #[inline]
    pub fn new_card_id(&mut self) -> u32 {
        self.next_card_id += 1;
        self.next_card_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PileKind { None, Hand, Draw, Discard, Exhaust, Play }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PilePos { None, Top, Bottom, Random }

#[derive(Clone, Copy, Debug)]
pub struct PlayResult { pub pile: PileKind, pub pos: PilePos, pub owner: CId }

/// r7 `_pending_play` frame.
#[derive(Clone, Debug)]
pub struct PendingPlay {
    pub card: KRef,
    pub target: Option<CId>,
    pub owner: CId,
    pub remaining: i32,
    pub play_count: i32,
    pub energy_spent: i32,
    pub energy_value: i32,
    pub force_exhaust: bool,
    pub is_auto: bool,
    pub result: PlayResult,
    pub awaiting_after_hook: bool,
    pub(crate) after_play_walk: Option<crate::hooks::AfterPlayWalk>,
    pub(crate) exhaust_caller: Option<crate::content::exhaust_calls::ExhaustCaller>,
    pub(crate) result_started: bool,
    pub(crate) result_added: bool,
    pub team_draw_after_owner: Option<(CId, i32)>,
    /// Work that C# performs after an awaited card draw resumes.  Modded is the
    /// current case: AddSlots -> await Draw -> AddThisCombat(1).
    pub(crate) post_draw_cost_card: Option<KRef>,
    pub(crate) post_shuffle_draw: Option<i32>,
    pub(crate) post_draw_cards: Option<Vec<KRef>>,
    pub(crate) post_draw_discard_card: Option<KRef>,
    pub(crate) post_draw_constellation: Option<(KRef,CId)>,
    pub(crate) post_draw_power: Option<(CId,PowerId,i32)>,
}

#[derive(Clone, Debug)]
pub struct AttackCtx {
    pub attacker: Option<CId>,
    pub target: Option<CId>,
    pub props: Props,
    pub base_damage: Option<i32>,
    pub results: Vec<(CId, i32)>,
}

/// r7 turn-resolution resume points after a pending card choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStage {
    /// Every player creature's block clear and `AfterBlockCleared` (C# `StartTurn`).
    Block,
    /// `SetupPlayerTurn` of `Combat::setup_player`, from its energy reset on.
    Energy,
    BeforeHandDraw,
    CardBeforeHandDraw,
    BeforeHandDrawLate,
    Draw,
    AfterPlayerTurnStart,
    PostAfterPlayerTurnStart,
    AutoPrePlay { next_player: CId, before_play: bool },
    EnemyAfterMove { enemy: CId, mv: u8, next_index: usize },
    PlayerTurnEnd,
}

/// r7 `DamageResult` of the latest `apply_damage` (cards that read the result: Fisticuffs...).
#[derive(Clone, Copy, Debug, Default)]
pub struct DamageOut {
    pub blocked: i32,
    pub hp_lost: i32,
    pub overkill: i32,
}

impl DamageOut {
    /// `total_damage` (blocked + unblocked/HP lost).
    #[inline]
    pub fn total(&self) -> i32 {
        self.blocked + self.hp_lost
    }
}

/// r7 `record_damage_event` entry (`_damage_events_combat`).
#[derive(Clone, Copy, Debug)]
pub struct DamageEvent {
    pub dealer: Option<CId>,
    pub target: CId,
    pub props: Props,
    pub unblocked: i32,
}

/// r7 `BlockGainedEntry` fields read by UnmovablePower.
#[derive(Clone, Copy, Debug)]
pub struct BlockEvent {
    pub target: CId,
    pub props: Props,
    /// Identity of the in-flight card play, not the card instance. Replays get distinct tokens.
    pub play_token: Option<i32>,
}

/// Per-turn and per-combat histories r7 keeps as event lists (counts only where that is all
/// any reader needs).
#[derive(Clone, Debug, Default)]
pub struct History {
    #[cfg(test)]
    pub(crate) card_lifecycle: Vec<crate::piles::Lifecycle>,
    #[cfg(test)]
    pub(crate) pile_contents_on_change: Vec<(KRef, Vec<(KRef, PileKind)>)>,
    /// C# CombatHistory.Summoned entries, keyed by the summoning player.
    pub summons: Vec<(CId, i32)>,
    /// `OrbChanneled` history entries this combat, and the Lightning ones (Voltaic).
    pub orbs_channeled: i32,
    pub lightning_channeled: i32,
    /// Public-history projection of OrbChanneledEntry.Actor; gameplay retains
    /// its existing counters and never reads this observation-only accounting.
    pub(crate) public_lightning_channels: [i32; crate::player::MAX_PLAYERS],
    /// Iteration counts this turn's Status CardDrawnEntry records by Actor.
    pub status_drawn_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// DeathMarch reads CardDrawnEntry.Actor and excludes FromHandDraw entries.
    pub extra_draws_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// C# CardPlay.Player is immutable even when The Ball later changes Card.Owner.
    pub played_this_turn: Vec<(CId, KRef)>,
    pub played_combat: Vec<(CId, KRef)>,
    /// C# CardPlayFinishedEntry: actor, that actor's turn number, and mutable card identity.
    pub finished_plays: Vec<(CId, i32, KRef)>,
    /// C# CreatureAttackedEntry actors for this side turn (including fully blocked attacks).
    pub attacked_this_turn: Vec<CId>,
    /// CardPlayFinishedEntry.WasEthereal, snapshotted at finish rather than read from mutable cards.
    pub ethereal_plays_finished: [i32; crate::player::MAX_PLAYERS],
    /// CardPlayStartedEntry attack types/players are snapshots, not mutable card references.
    pub attack_starts_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// DeathsDoor reads PowerReceivedEntry.Applier, including zero-offset stacks.
    pub doom_appliers_this_turn: [bool; crate::player::MAX_PLAYERS],
    /// `CardPlayStartedEntry` count for the current side turn.
    // fire: core/combat.py:1839-1841,3696-3713
    pub play_starts_this_turn: i32,
    /// `_card_play_starts_this_turn` cards, in start order (FOLLOW_THROUGH reads the last one).
    pub play_start_cards: Vec<KRef>,
    /// C# CardPlayStartedEntry.IsFirstInSeries actors, in the current side turn.
    pub first_series_actors: Vec<CId>,
    /// `_discard_events_this_turn` length (solo play: every discarded card is the player's).
    // fire: core/combat.py:2786-2790
    pub discarded_this_turn: i32,
    pub discarded_by_owner_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// Card-exhaust history count for the current side turn.
    // fire: core/combat.py was_card_exhausted_this_turn
    pub exhausted_this_turn: i32,
    /// C# CombatHistory CardExhaustedEntry count, across all players and turns.
    pub exhausted_combat: i32,
    pub cards_drawn_combat: i32,
    pub drawn_by_owner_combat: [i32; crate::player::MAX_PLAYERS],
    /// Per owner: HelixDrill sums EnergySpentEntry rows whose actor is the card owner.
    pub energy_spent_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// `_generated_cards_combat` length.
    pub generated_combat: i32,
    /// `_generated_cards_combat` entries with `added_by_player` (`count_generated_cards_this_combat`).
    pub generated_by_player_combat: i32,
    /// CardGeneratedEntry.Creator, independently of the generated card's owner.
    pub generated_by_creator: [i32; crate::player::MAX_PLAYERS],
    /// Radiate.cs:26: positive StarsModifiedEntry amounts filtered by Actor.
    pub stars_gained_this_turn: [i32; crate::player::MAX_PLAYERS],
    /// `_damage_events_combat`; this side-turn's events are `damage[damage_turn_start..]`
    /// (r7 keeps a parallel per-turn list holding exactly those).
    pub damage: Vec<DamageEvent>,
    pub damage_turn_start: usize,
    /// EmotionChip reads DamageReceivedEntry's immutable receiver turn and
    /// WasFullyBlocked, including unblocked zero damage and Osty redirection.
    pub damage_received_turns: Vec<(CId, i32, bool)>,
    /// r7 `_block_events_this_turn`.
    pub block_this_turn: Vec<BlockEvent>,
}

impl History {
    /// `_reset_side_turn_history` (damage part).
    #[inline]
    pub fn reset_side_turn(&mut self) {
        self.damage_turn_start = self.damage.len();
        self.attacked_this_turn.clear();
        self.attack_starts_this_turn.fill(0);
        self.doom_appliers_this_turn.fill(false);
        self.play_starts_this_turn = 0;
        self.play_start_cards.clear();
        self.first_series_actors.clear();
        self.status_drawn_this_turn.fill(0);
        self.extra_draws_this_turn.fill(0);
        self.discarded_this_turn = 0;
        self.discarded_by_owner_this_turn.fill(0);
        self.exhausted_this_turn = 0;
        self.stars_gained_this_turn.fill(0);
        self.block_this_turn.clear();
    }

    #[inline]
    pub fn damage_this_turn(&self) -> &[DamageEvent] {
        &self.damage[self.damage_turn_start..]
    }

    /// `count_unblocked_hits_received_this_combat`.
    pub fn unblocked_hits_received_combat(&self, target: CId) -> i32 {
        self.damage.iter().filter(|d| d.target == target && d.unblocked > 0).count() as i32
    }

    /// `count_powered_hits_this_turn(dealer, target)`.
    pub fn powered_hits_this_turn(&self, dealer: CId, target: CId) -> i32 {
        self.damage_this_turn().iter()
            .filter(|d| d.dealer == Some(dealer) && d.target == target && d.props.is_powered_attack()).count() as i32
    }
}

impl Combat {
    /// Persistent gold belongs to the effect's player, never implicitly to seat zero.
    pub(crate) fn change_player_gold(&mut self, owner: CId, delta: i32) {
        let delta = if delta > 0 { self.relic_modify_gold_gained(owner, delta) } else { delta };
        if owner == PLAYER {
            self.run.gold = (self.run.gold + delta).max(0);
        } else if let Some(gold) = self.players[owner].gold {
            self.players[owner].gold = Some((gold + delta).max(0));
        } else {
            self.unported("gold change requires target player's initial gold");
        }
        if delta > 0 && self.fault.is_none() {
            self.relic_after_gold_gained(owner);
        }
    }

    /// C# GremlinMerc Gimme/DoubleSmash/Hehe: Steal once after the entire attack,
    /// not once per hit or as a general AfterAttack power hook.
    pub(crate) fn gremlin_merc_steal(&mut self, owner: CId) {
        let powers: Vec<_> = self.cr(owner).power_list().iter().copied()
            .filter(|&p| self.powers[p as usize].id == PowerId::Thievery).collect();
        for p in powers {
            // C# ThieveryPower.Steal / PlayerCmd.LoseGold(Stolen).
            let Some(target) = self.powers[p as usize].target else { continue };
            if self.cr(target).is_dead() { continue; }
            let gold = if target == PLAYER { Some(self.run.gold) } else { self.players[target].gold };
            let Some(gold) = gold else {
                self.unported("THIEVERY requires target player's initial gold");
                return;
            };
            if gold <= 0 { continue; }
            let stolen = self.powers[p as usize].amount.min(gold);
            if target == PLAYER {
                self.run.gold = (gold - stolen).max(0);
            } else {
                self.players[target].gold = Some((gold - stolen).max(0));
            }
            self.powers[p as usize].vars[0] += stolen;
        }
    }

    /// `has_unblocked_damage_received_this_turn(target, side=...)`.
    pub fn has_unblocked_damage_received_this_turn(&self, target: CId, side: Option<Side>) -> bool {
        if side.is_some_and(|s| s != self.side) {
            return false;
        }
        self.hist.damage_this_turn().iter().any(|d| d.target == target && d.unblocked > 0)
    }

    #[inline]
    pub(crate) fn card_play_token(&self, card: Option<KRef>) -> Option<i32> {
        let k = card?;
        if self.active_card == Some(k)
            || self.pending_play.last().is_some_and(|frame| frame.card == k)
        {
            Some(self.hist.play_starts_this_turn)
        } else {
            None
        }
    }

    /// `record_block_gained_event`; callers invoke this only for positive actual block gains.
    pub(crate) fn record_block_gained_event(
        &mut self,
        target: CId,
        props: Props,
        card: Option<KRef>,
    ) {
        self.hist.block_this_turn.push(BlockEvent {
            target,
            props,
            play_token: self.card_play_token(card),
        });
    }
}

/// Owned rewards emitted by combat listeners, consumed by the run layer later.
#[derive(Clone, Debug)]
pub enum EndReward {
    CardRemoval { owner: CId },
    SpecialCard { owner: CId, card: Card },
    Gold { owner: CId, amount: i32 },
    Card { owner: CId, room: RoomType },
}

#[derive(Clone, Debug)]
pub struct Combat {
    pub action_queue: crate::action_queue::CombatQueue,
    pub between_instance: u64,
    pub(crate) orb_recording: bool,
    pub(crate) next_orb_instance: u64,
    /// Optional read-only oracle probe at presentation, after the live hook
    /// action starts and its candidate list has been constructed.
    pub on_choice_presented: Option<fn(&Combat)>,
    pub(crate) live_choices: Vec<crate::choice::LiveChoiceFrame>,
    pub(crate) live_action_depth: usize,
    pub(crate) pending_player_turn_hooks: Option<crate::hooks::TurnStartWalk>,
    pub(crate) pending_turn_end_hooks: Option<crate::hooks::TurnEndWalk>,
    pub(crate) turn_end_join: Option<crate::hooks::TurnEndWalk>,
    pub(crate) collect_setup_choices: bool,
    pub(crate) signalled_setup_choice: Option<crate::choice::DeferredSetupChoice>,
    // None walk: a branched card draw, queued behind the parent action.
    pub(crate) deferred_setup_choices: Vec<(CId, Option<crate::hooks::TurnStartWalk>, crate::choice::DeferredSetupChoice)>,
    pub(crate) setup_enchant_pending: [bool; crate::player::MAX_PLAYERS],
    pub(crate) setup_auto_pre_play_done: [bool; crate::player::MAX_PLAYERS],
    /// Zero means the ordinary whole-party turn; otherwise only these seats play.
    pub extra_turn_players: u8,
    /// C# `CombatTurnState.PlayersReadyToEndTurn`, one bit per seated player.
    pub(crate) players_ready_to_end_turn: u8,
    /// C# `CombatTurnState.PlayersReadyToBeginEnemyTurn`, one bit per seated player.
    pub(crate) players_ready_to_begin_enemy_turn: u8,
    /// Phase one has completed; C# can now accept `ReadyToBeginEnemyTurnAction`s.
    pub(crate) player_turn_phase_one_done: bool,
    /// Compatibility path for the oracle's legacy all-seats-at-once `end_player_turn()`.
    pub(crate) auto_begin_enemy_turn: bool,
    pub(crate) pending_live_setup: Option<(CId, TurnStage)>,
    pub rng: Rng,
    pub run: RunCtx,
    pub creatures: Vec<Creature>,
    // relic fire: core/combat.py:278
    pub allies: ArrayVec<CId, MAX_PET_ALLIES>,
    pub enemies: ArrayVec<CId, 8>,
    pub powers: Vec<Power>,
    pub cards: Vec<Card>,
    /// C# `CombatState.Players` combat states (piles, energy, stars, orbs), by creature id.
    pub players: crate::player::Players,
    pub round: i32,
    pub side: Side,
    pub turn_count: i32,
    pub is_over: bool,
    pub is_ending: bool,
    pub won: bool,
    pub combat_started: bool,
    /// C# GremlinMercNormal.MarkGoldStolen, set by SurprisePower.AfterDeath.
    pub gremlin_gold_stolen: bool,
    /// Player.ExtraFields.DamageDealt. Counts unblocked HP loss from direct
    /// player dealers to non-player receivers, excluding overkill.
    pub player_damage_dealt: [i32; crate::player::MAX_PLAYERS],
    pub player_debuffs_applied: [i32; crate::player::MAX_PLAYERS],
    pub escaped_enemies: Vec<CId>,
    /// Pending rewards emitted at their AfterCombatEnd listener positions.
    pub end_rewards: Vec<EndReward>,
    pub in_play_phase: bool,
    pub end_turn_after_play: bool,
    pub pending_play: Vec<PendingPlay>,
    /// MonologuePower's per-instance dictionary of cards awaiting AfterCardPlayed.
    pub(crate) monologue_plays: Vec<(PRef, KRef)>,
    /// Awaited Eidolon snapshot: parent play depth and remaining cards.
    pub(crate) autoplay_snapshots: Vec<(usize, crate::autoplay::AutoplayWork)>,
    pub(crate) pending_draws: Vec<(usize, CId, i32, bool, Option<usize>)>,
    pub(crate) pending_drawn_hooks: Vec<(usize, KRef, bool)>,
    pub(crate) pending_exhaust_walks: Vec<(usize, crate::hooks::ExhaustWalk)>,
    pub(crate) pending_damage_walks: Vec<(usize, crate::hooks::DamageReceivedWalk)>,
    pub(crate) pending_puzzle_draws: Vec<(usize, CId, i32)>,
    pub(crate) pending_monster_attacks: Vec<(usize, crate::damage::MonsterAttackWalk)>,
    pub(crate) collect_damage_callbacks: Option<Vec<crate::damage::DamageCallback>>,
    pub(crate) pending_shuffle_relics: Option<CId>,
    /// Owner-blocking potion effect: play depth, inventory owner, potion, target.
    pub(crate) pending_potion_after: Option<(usize, CId, PotionId, Option<CId>)>,
    pub damage_depth: i32,
    pub end_defer_depth: i32,
    pub next_enemy_combat_id: u8,
    /// Only offline room-entry copies defer AfterAddedToRoom starting powers.
    pub(crate) replay_room_entry: bool,
    pub attack_stack: Vec<AttackCtx>,
    pub(crate) completed_attack_results: Vec<(CId, i32)>,
    pub(crate) stolen_power_amounts: Vec<(PRef, CId, i32)>,
    pub pending_auto_attack: bool,
    pub active_card: Option<KRef>,
    pub active_target: Option<CId>,
    pub hist: History,
    pub encounter_slots: &'static [&'static str],
    pub fault: Option<&'static str>,
    /// Result of the latest `apply_damage`.
    pub last_damage: DamageOut,
    /// r7 `_pending_turn_setup`: the stage to resume once the pending choice resolves.
    pub pending_turn_setup: Option<TurnStage>,
    /// The player whose `SetupPlayerTurn` the per-player turn stages run for (C# runs the
    /// setups in `Players` order); a paused setup resumes with the same player.
    pub setup_player: CId,
    /// OR of `CARD_HOOKS` over every card in the arena (cards are never removed from it).
    pub card_hook_mask: u32,
    /// Card whose effect or hook raised `fault` (diagnostics only).
    pub fault_card: Option<CardId>,
    /// Power whose application raised `fault` (diagnostics only).
    pub fault_power: Option<PowerId>,
    /// Relic that made Combat::new fail closed (diagnostics only).
    pub fault_relic: Option<RelicId>,
    /// Proof-only snapshots taken inside HexPower.AfterRemoved; never encoded.
    #[cfg(feature = "selector-proof")]
    pub hex_removal_proof: Vec<(CId, Vec<u32>, Vec<(CId, u32, Option<crate::card::CardAffliction>, i32)>,
        Vec<(CId, u32, Option<crate::card::CardAffliction>, i32)>)>,
    /// Thrash id, exhausted id, owner, before, C# after, legacy after.
    /// Only captured for PenNib counter 9 / no AttackToDouble, at completed exhaust.
    #[cfg(feature = "selector-proof")]
    pub thrash_pen_nib_proof: Vec<(u32,u32,CId,i32,i32,i32)>,
    /// MadScience id, preserved base_block, legacy upgraded-factory base_block.
    #[cfg(feature = "selector-proof")]
    pub mad_science_upgrade_proof: Vec<(u32,Option<i32>,Option<i32>)>,
    /// r7 `pending_choice`; options and selection live in reused buffers.
    pub pending_choice: Option<crate::choice::PendingChoice>,
    pub(crate) pending_choice_orb_context: Option<crate::between::OrbContext>,
    pub(crate) pending_choice_orb_spans: Vec<u64>,
    pub choice_options: Vec<KRef>,
    pub choice_selected: Vec<u16>,
    /// r7 `_amounts_for_played_cards` of Afterimage/Serpent Form: (owner, power, card, amount).
    pub power_card_amounts: Vec<(CId, PowerId, KRef, i32)>,
    /// NightmarePower `_selected_cards`: (owner, hidden clone, copies).
    pub nightmare_cards: Vec<(CId, KRef, i32)>,
    /// C# ImitationLearningPower.Data: source/clone pairs per watching instance.
    pub imitation_clones: Vec<(PRef, KRef, KRef)>,
    /// OblivionPower.Data.amountsForPlayedCards, scoped to each per-applier instance.
    pub oblivion_cards: Vec<(PRef, KRef, i32)>,
    /// StranglePower.Data.amountsForPlayedCards, scoped to each per-applier instance.
    pub strangle_cards: Vec<(PRef, KRef, i32)>,
    /// `from_hand_draw` of the draw whose `on_card_drawn` listeners are running.
    pub drawing_from_hand: bool,
    /// Set right before an `apply_damage` call whose kill the caller performs itself
    /// (`CreatureCmd.Damage` damages every target first, then `Kill(killedCreatures)`); the call
    /// takes it on entry, so damage nested inside that hit still kills at once.
    pub(crate) defer_kill: bool,
}

impl Combat {
    // ───────────────────────────── construction ─────────────────────────────

    /// `CombatState.__init__` + `_create_combat_player_state`: clone every persistent deck card
    /// into the combat arena with fresh instance ids.
    pub fn new(run: RunCtx, combat_seed: u64) -> Combat {
        Self::new_for_character(run, combat_seed, Character::Ironclad)
    }

    /// C# Player.Character survives entry into combat, independently of deck colors.
    pub fn new_for_character(run: RunCtx, combat_seed: u64, character: crate::character::Character) -> Combat {
        Self::new_with_deck(run,combat_seed,character,true)
    }

    pub(crate) fn new_event_layout(run:RunCtx,combat_seed:u64,character: crate::character::Character)->Combat {
        Self::new_with_deck(run,combat_seed,character,false)
    }

    fn clone_player_deck(run:&mut RunCtx)->Vec<Card> {
        let mut cards = Vec::with_capacity(run.deck.len() + 16);
        for (i, deck_card) in run.deck.iter().enumerate() {
            let id = {
                run.next_card_id += 1;
                run.next_card_id
            };
            let mut c = deck_card.clone_with_id(id);
            if c.id == CardId::TheScythe {
                c.cv.scythe_growth = deck_card.damage.unwrap_or(13) - 13;
            }
            c.cv.is_clone = false;
            c.deck_version = Some(i as u16);
            c.deck_owner = PLAYER;
            cards.push(c);
        }
        cards
    }

    pub(crate) fn initialize_prepared_player_deck(&mut self) {
        assert!(self.cards.is_empty()&&self.players[PLAYER].starting_deck.is_empty());
        self.cards=Self::clone_player_deck(&mut self.run);
        self.players[PLAYER].starting_deck.extend((0..self.cards.len()).map(|i|i as KRef));
        self.refresh_card_arena_caches();
    }

    fn refresh_card_arena_caches(&mut self) {
        self.card_hook_mask=self.cards.iter().fold(0,|mask,card|mask|CARD_HOOKS[card.id.index()]);
    }

    fn new_with_deck(mut run:RunCtx,combat_seed:u64,character: crate::character::Character,copy_deck:bool)->Combat {
        let cards=if copy_deck {Self::clone_player_deck(&mut run)} else {Vec::new()};
        let base_max_energy = if run.max_energy > 0 { run.max_energy } else { BASE_ENERGY };
        let mut solo = crate::player::PlayerCombatState::new(PLAYER, base_max_energy, run.deck.len());
        solo.character = Some(character);
        solo.orbs.open_base_slots(run.base_orb_slots);
        solo.starting_deck.extend((0..cards.len()).map(|i|i as KRef));
        let player = Creature::player(run.max_hp, run.hp);
        let mut players = crate::player::Players::default();
        players.0.push(solo);
        let between_instance = crate::between::combat_created();
        let mut combat = Combat {
            action_queue: crate::action_queue::CombatQueue::default(),
            between_instance,
            orb_recording: between_instance != 0,
            next_orb_instance: 0,
            on_choice_presented: None,
            live_choices: Vec::new(),
            live_action_depth: 0,
            pending_player_turn_hooks: None,
            pending_turn_end_hooks: None,
            turn_end_join: None,
            collect_setup_choices: false,
            setup_auto_pre_play_done: [false; crate::player::MAX_PLAYERS],
            extra_turn_players: 0,
            players_ready_to_end_turn: 0,
            players_ready_to_begin_enemy_turn: 0,
            player_turn_phase_one_done: false,
            auto_begin_enemy_turn: false,
            pending_live_setup: None,
            signalled_setup_choice: None,
            deferred_setup_choices: Vec::new(),
            setup_enchant_pending: [false; crate::player::MAX_PLAYERS],
            rng: Rng::new(combat_seed),
            run,
            creatures: vec![player],
            // relic fire: core/combat.py:278
            allies: ArrayVec::new(),
            enemies: ArrayVec::new(),
            powers: Vec::with_capacity(32),
            cards,
            players,
            round: 1,
            side: Side::Player,
            turn_count: 0,
            is_over: false,
            is_ending: false,
            won: false,
            combat_started: false,
            gremlin_gold_stolen: false,
            player_damage_dealt: [0; crate::player::MAX_PLAYERS],
            player_debuffs_applied: [0; crate::player::MAX_PLAYERS],
            escaped_enemies: Vec::new(),
            end_rewards: Vec::new(),
            in_play_phase: false,
            end_turn_after_play: false,
            pending_play: Vec::with_capacity(4),
            monologue_plays: Vec::new(),
            autoplay_snapshots: Vec::new(),
            pending_draws: Vec::new(),
            pending_drawn_hooks: Vec::new(),
            pending_exhaust_walks: Vec::new(),
            pending_damage_walks: Vec::new(),
            pending_puzzle_draws: Vec::new(),
            pending_monster_attacks: Vec::new(),
            collect_damage_callbacks: None,
            pending_shuffle_relics: None,
            pending_potion_after: None,
            damage_depth: 0,
            end_defer_depth: 0,
            next_enemy_combat_id: 0,
            replay_room_entry: false,
            attack_stack: Vec::with_capacity(4),
            completed_attack_results: Vec::new(),
            stolen_power_amounts: Vec::new(),
            pending_auto_attack: false,
            active_card: None,
            active_target: None,
            hist: History::default(),
            encounter_slots: &[],
            fault: None,
            card_hook_mask: 0,
            last_damage: DamageOut::default(),
            pending_turn_setup: None,
            setup_player: 0,
            fault_card: None,
            fault_power: None,
            fault_relic: None,
            #[cfg(feature = "selector-proof")]
            hex_removal_proof: Vec::new(),
            #[cfg(feature = "selector-proof")]
            thrash_pen_nib_proof: Vec::new(),
            #[cfg(feature = "selector-proof")]
            mad_science_upgrade_proof: Vec::new(),
            pending_choice: None,
            pending_choice_orb_context: None,
            pending_choice_orb_spans: Vec::new(),
            choice_options: Vec::with_capacity(48),
            choice_selected: Vec::with_capacity(16),
            power_card_amounts: Vec::new(),
            nightmare_cards: Vec::new(),
            imitation_clones: Vec::new(),
            oblivion_cards: Vec::new(),
            strangle_cards: Vec::new(),
            drawing_from_hand: false,
            defer_kill: false,
        };
        combat.refresh_card_arena_caches();
        for r in &combat.run.relics {
            if !crate::relic::is_ported(r.id) {
                if combat.fault.is_none() {
                    combat.fault_relic = Some(r.id);
                }
                combat.fault = Some("unported relic");
            }
        }
        combat
    }

    #[cold]
    pub fn unported(&mut self, what: &'static str) {
        if self.fault.is_none() {
            self.fault = Some(what);
        }
    }

    /// Core/DevConsole/ConsoleCommands/WinConsoleCmd.cs: KillEnemies.
    /// Not a policy action. No player HP or inventory is granted.
    pub fn debug_win_combat(&mut self)->Result<(),&'static str> {
        if !self.combat_started||self.is_over {return Err("debug win requires combat");}
        let enemies=self.enemies.clone();
        for enemy in enemies {
            for power in self.cr(enemy).power_list() {
                self.remove_power_instance_raw(enemy,power);
            }
            self.kill_creature(enemy);
        }
        self.check_combat_end();
        self.fault.map_or(Ok(()),Err)
    }

    /// Seat another player (C# `CombatState.Players` order): its creature takes the next
    /// creature id, so it must join before any ally or enemy. `deck` is cloned into the
    /// arena with fresh instance ids, like `Combat::new` does for the first player.
    pub fn add_player(&mut self, max_hp: i32, hp: i32, base_max_energy: i32, base_orb_slots: i32, deck: &[Card]) -> CId {
        assert!(
            self.creatures.len() == self.players.len() && self.players.len() < crate::player::MAX_PLAYERS,
            "players join before allies and enemies, at most MAX_PLAYERS"
        );
        let owner = self.creatures.len() as CId;
        let mut state = crate::player::PlayerCombatState::new(owner, base_max_energy, deck.len());
        // Player.CreateForNewRun passes initialMaxPotionSlotCount = 3; loaders
        // overwrite this fixed empty inventory with the recorded capacity/contents.
        // AscensionManager.ApplyEffectsTo / TightBelt (A4) removes one slot.
        state.potions = Some(vec![None; if self.run.ascension >= 4 { 2 } else { 3 }]);
        state.persistent_deck = deck.to_vec();
        for card in &mut state.persistent_deck {
            card.owner = owner;
        }
        state.orbs.open_base_slots(base_orb_slots);
        for (i, deck_card) in deck.iter().enumerate() {
            let iid = self.run.new_card_id();
            let mut c = deck_card.clone_with_id(iid);
            if c.id == CardId::TheScythe {
                c.cv.scythe_growth = deck_card.damage.unwrap_or(13) - 13;
            }
            c.cv.is_clone = false;
            c.deck_version = Some(i as u16);
            c.deck_owner = owner;
            c.owner = owner;
            // Secondary seats' starting cards participate in the same hook fast path.
            self.card_hook_mask |= CARD_HOOKS[c.id.index()];
            state.starting_deck.push(self.cards.len() as KRef);
            self.cards.push(c);
        }
        self.creatures.push(Creature::player(max_hp, hp));
        self.players.0.push(state);
        owner
    }

    /// The player's original deck, separate from the mutable combat card copies.
    pub fn player_deck(&self, owner: CId) -> &[Card] {
        if owner == PLAYER { &self.run.deck } else { &self.players[owner].persistent_deck }
    }

    #[cfg(test)]
    pub(crate) fn player_deck_mut(&mut self, owner: CId) -> &mut Vec<Card> {
        if owner == PLAYER { &mut self.run.deck } else { &mut self.players[owner].persistent_deck }
    }
    /// Production callers may mutate card values, never erase object references.
    #[cfg(not(test))]
    pub(crate) fn player_deck_mut(&mut self,owner:CId)->&mut [Card] {
        if owner==PLAYER {&mut self.run.deck} else {&mut self.players[owner].persistent_deck}
    }
    pub(crate) fn push_player_deck_card(&mut self,owner:CId,card:Card) {
        if owner==PLAYER {self.run.deck.push(card);} else {self.players[owner].persistent_deck.push(card);}
    }
    pub(crate) fn remove_player_deck_card(&mut self,owner:CId,index:usize)->Card {
        if owner==PLAYER {self.run.remove_run_deck_card(index)}
        else {crate::relic::remove_deck_object(&mut self.players[owner].persistent_deck,&mut self.run.relics,index)}
    }
    pub(crate) fn retain_player_deck_cards(&mut self,owner:CId,predicate:impl FnMut(&Card)->bool) {
        if owner==PLAYER {self.run.retain_run_deck_cards(predicate);}
        else {crate::relic::retain_deck_objects(&mut self.players[owner].persistent_deck,&mut self.run.relics,predicate);}
    }

    #[inline] pub fn player(&self) -> &Creature { &self.creatures[PLAYER as usize] }

    /// C# GremlinMercNormal.CalculateGoldProportion.
    pub fn gold_proportion(&self) -> f64 {
        if !self.escaped_enemies.iter().any(|&e| self.cr(e).monster_id == "FAT_GREMLIN") {
            1.0
        } else if self.gremlin_gold_stolen {
            0.0
        } else {
            0.5
        }
    }

    /// Living player creatures in `Players` order (a monster move's player `targets`).
    pub fn living_players(&self) -> ArrayVec<CId, { crate::player::MAX_PLAYERS }> {
        (0..self.players.len() as CId).filter(|&p| self.cr(p).is_alive()).collect()
    }
    #[inline] pub fn cr(&self, c: CId) -> &Creature { &self.creatures[c as usize] }
    #[inline] pub fn cr_mut(&mut self, c: CId) -> &mut Creature { &mut self.creatures[c as usize] }

    /// `CombatState.Creatures`: `_allies` (every seated player in `Players` order, then pets and
    /// other allies) + enemies. A player whose death finished is skipped: its hooks are off
    /// (`Player.DeactivateHooks` at the end of `CreatureCmd.KillWithoutCheckingWinCondition`).
    #[inline]
    pub fn all_creatures(&self) -> ArrayVec<CId, MAX_COMBAT_CREATURES> {
        let mut out = ArrayVec::new();
        out.extend((0..self.players.len() as CId).filter(|&p| {
            let cr = self.cr(p);
            !(cr.is_dead() && cr.death_processed)
        }));
        // relic fire: core/combat.py:578
        out.extend(self.allies.iter().copied());
        out.extend(self.enemies.iter().copied());
        out
    }

    pub fn alive_enemies(&self) -> ArrayVec<CId, 8> {
        self.enemies.iter().copied().filter(|&e| self.cr(e).is_alive()).collect()
    }

    fn powers_allow_hitting(&self, c: CId) -> bool {
        // C# DieForYouPower.ShouldAllowHitting is a combat-wide hook, not an
        // owner-only veto: it returns creature.IsAlive for EVERY proposed target.
        // The power intentionally survives Osty's death, so its veto does too.
        if self.cr(c).is_dead() && self.all_creatures().into_iter()
            .any(|owner| self.has_power(owner, PowerId::DieForYou))
        {
            return false;
        }
        for p in self.cr(c).powers.iter().copied() {
            if !self.powers[p as usize].meta().listens(Hook::ShouldAllowHitting) {
                continue;
            }
            let id = self.powers[p as usize].id;
            if matches!(
                id,
                PowerId::Reattach | PowerId::Adaptable | PowerId::Illusion | PowerId::DoorRevival
            )
                && self.power_private_int(c, id, 0) != 0
            {
                return false;
            }
        }
        true
    }

    pub fn can_hit(&self, c: CId) -> bool {
        self.cr(c).is_alive() && self.powers_allow_hitting(c)
    }

    pub fn hittable_enemies(&self) -> ArrayVec<CId, 8> {
        self.enemies.iter().copied().filter(|&e| self.can_hit(e)).collect()
    }

    /// `CombatManager.IsOverOrEnding`.
    pub fn is_over_or_ending(&self) -> bool {
        self.is_over || self.is_ending || self.is_combat_ending_live()
    }

    pub(crate) fn is_combat_ending_live(&self) -> bool {
        if !self.combat_started || self.is_over {
            return false;
        }
        if self.enemies.iter().any(|&e| self.cr(e).is_alive() && !self.has_power(e, PowerId::Minion)) {
            return false;
        }
        !self.enemies.iter().copied().any(|e| {
            if self.cr(e).is_alive() || self.has_power(e, PowerId::Minion) {
                return false;
            }
            self.cr(e).powers.iter().copied().any(|p| match self.powers[p as usize].id {
                PowerId::Adaptable => true,
                PowerId::Illusion => self.power_private_int(e, PowerId::Illusion, 0) != 0,
                // powers/remaining_c.py:1045-1046 StockPower.should_stop_combat_ending.
                PowerId::Stock => true,
                PowerId::Surprise => self.powers[p as usize].payload == 0,
                // powers/monster.py:1401-1412 DoorRevivalPower.should_stop_combat_ending.
                PowerId::DoorRevival => {
                    self.power_private_int(e, PowerId::DoorRevival, 0) != 0
                        && self.enemies.iter().copied().any(|x| {
                            self.cr(x).monster_id == "DOORMAKER" && self.cr(x).is_alive()
                        })
                }
                _ => false,
            })
        })
    }

    // ───────────────────────────── powers ─────────────────────────────

    pub fn find_power(&self, c: CId, id: PowerId) -> Option<PRef> {
        self.cr(c).powers.iter().copied().find(|&p| self.powers[p as usize].id == id)
    }
    pub fn power_amount(&self, c: CId, id: PowerId) -> i32 {
        self.find_power(c, id).map_or(0, |p| self.powers[p as usize].amount)
    }
    pub fn has_power(&self, c: CId, id: PowerId) -> bool {
        self.find_power(c, id).is_some_and(|p| self.powers[p as usize].amount != 0)
    }

    /// `_power_type_for_amount`.
    pub(crate) fn power_type_for_amount(id: PowerId, amount: i32) -> PowerType {
        let m = power_meta(id);
        if m.stack_type == PowerStackType::Counter && m.allow_negative && amount < 0 {
            return PowerType::Debuff;
        }
        if !m.allow_negative && m.power_type == PowerType::Debuff && amount < 0 {
            return PowerType::Buff;
        }
        m.power_type
    }

    /// `Creature.remove_power_instance`.
    fn remove_power_instance_raw(&mut self, owner: CId, p: PRef) -> bool {
        let id = self.powers[p as usize].id;
        let cr = &mut self.creatures[owner as usize];
        if let Some(pos) = cr.powers.iter().position(|&q| q == p) {
            let replacement = cr.extra.iter().position(|&q| self.powers[q as usize].id == id);
            match replacement {
                None => {
                    cr.powers.remove(pos);
                }
                Some(r) => {
                    let q = cr.extra.remove(r);
                    cr.powers[pos] = q;
                }
            }
            self.record_public_power(p,"remove",Some(self.powers[p as usize].amount),false,false);
            return true;
        }
        if let Some(pos) = cr.extra.iter().position(|&q| q == p) {
            cr.extra.remove(pos);
            self.record_public_power(p,"remove",Some(self.powers[p as usize].amount),false,false);
            return true;
        }
        false
    }

    /// `CombatState._remove_power_instance` (fires `on_removed`).
    pub fn remove_power(&mut self, owner: CId, p: PRef) {
        if self.remove_power_instance_raw(owner, p) {
            #[cfg(feature = "selector-proof")]
            let before = (self.powers[p as usize].id == PowerId::Hex).then(|| (
                self.pile_cards(owner).iter().map(|&k|self.cards[k as usize].instance_id).collect(),
                self.cards.iter().map(|c|
                    (c.owner,c.instance_id,c.cv.affliction,c.cv.affliction_amount)).collect()));
            self.power_on_removed(p, owner);
            #[cfg(feature = "selector-proof")]
            if let Some((members,before)) = before {
                self.hex_removal_proof.push((owner,members,before,self.cards.iter().map(|c|
                    (c.owner,c.instance_id,c.cv.affliction,c.cv.affliction_amount)).collect()));
            }
        }
    }

    /// `Creature.apply_power`.
    pub fn creature_apply_power(&mut self, target: CId, id: PowerId, amount: i32, applier: Option<CId>,
                                source: Option<KRef>, ignore_next: bool, force_new: bool) -> Option<PRef> {
        self.apply_power_instance(target, id, amount, applier, source, ignore_next, force_new, None, None)
    }

    /// C# PowerCmd.ModifyAmount: change this exact instanced power, retaining all
    /// amount hooks instead of Apply's search for a stackable instance.
    pub(crate) fn modify_power_instance(&mut self, p: PRef, amount: i32, applier: Option<CId>, source: Option<KRef>) {
        if self.is_over_or_ending() {
            return;
        }
        let power = &self.powers[p as usize];
        self.apply_power_instance(power.owner, power.id, amount, applier, source, false, false, Some(p), None);
    }

    /// Misery clones the original class instance BEFORE applying it. New instances
    /// retain private counters/target/temporary state; existing stacks retain theirs.
    pub(crate) fn apply_cloned_power(&mut self, target: CId, power: &Power, amount: i32, source: KRef) {
        self.apply_power_instance(target, power.id, amount, power.applier, Some(source),
            false, false, None, Some(power.clone()));
    }

    /// Audited TemporaryStrengthPower subclasses with IsPositive == false.
    pub(crate) fn is_temporary_strength_loss(id: PowerId) -> bool {
        matches!(id, PowerId::CrushUnder | PowerId::DarkShackles | PowerId::DyingStar |
            PowerId::EnfeeblingTouch | PowerId::Mangle | PowerId::MonarchsGazeStrengthDown |
            PowerId::PiercingWail | PowerId::ShacklingPotion)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_power_instance(&mut self, target: CId, id: PowerId, mut amount: i32, applier: Option<CId>,
                            source: Option<KRef>, ignore_next: bool, force_new: bool, instance: Option<PRef>, cloned: Option<Power>) -> Option<PRef> {
        // MagicBombPower.InstanceType is Instanced (MagicBombPower.cs:19).
        let force_new = force_new || id == PowerId::MagicBomb && instance.is_none();
        let stacking_doom = id == PowerId::Doom && !force_new && self.find_power(target, id).is_some();
        let original = amount;
        // PowerCmd.Apply/ModifyAmount select the actual model BEFORE the
        // before-amount hook. Reserve a fresh, unattached arena instance so a
        // relic can retain that identity across nested applications/removal.
        let existing = instance.or_else(|| {
            if force_new { return None; }
            if matches!(id, PowerId::Oblivion | PowerId::Strangle) {
                return self.cr(target).power_list().into_iter().find(|&p|
                    self.powers[p as usize].id == id && self.powers[p as usize].applier == applier);
            }
            if power_meta(id).instanced { None } else { self.find_power(target, id) }
        });
        let changing = existing.unwrap_or_else(|| {
            let p = self.powers.len() as PRef;
            let mut power = cloned.unwrap_or(Power { id, amount: 0, payload: 0, skip_next_tick: false,
                applier, ignore_next_instance: ignore_next, owner: target, vars: [0; 4], target: None });
            power.owner = target;
            power.applier = applier;
            self.powers.push(power);
            p
        });
        self.fire_before_power_amount_changed(target, id, amount, applier, source, changing);
        if applier.is_some() {
            // relic fire: relics/rare.py:1138
            amount = self.modify_power_amount_given(id, amount, applier, target, source);
            let _ = original;
        }
        amount = self.modify_power_amount_received(id, amount, target, applier);
        // PowerCmd.ModifyAmount records PowerReceived even when Artifact reduces the offset
        // to zero; a fresh Apply records only nonzero power application.
        if stacking_doom && let Some(p) = applier.filter(|&p| self.players.is_player(p)) {
            self.hist.doom_appliers_this_turn[p as usize] = true;
        }
        if amount == 0 {
            return None;
        }
        let meta = power_meta(id);
        if !meta.exists {
            self.unported("power class missing in r7");
            return None;
        }
        let is_debuff = Self::power_type_for_amount(id, amount) == PowerType::Debuff && meta.visible;
        if is_debuff && let Some(art) = self.find_power(target, PowerId::Artifact) {
            if self.powers[art as usize].amount > 0 {
                let before=self.powers[art as usize].amount;
                self.powers[art as usize].amount -= 1;
                self.record_public_power(art,"change",Some(before),true,false);
                if self.powers[art as usize].amount <= 0 {
                    // r7 deletes the dict entry directly here (no on_removed).
                    let cr = &mut self.creatures[target as usize];
                    if let Some(pos) = cr.powers.iter().position(|&q| q == art) {
                        cr.powers.remove(pos);
                    }
                    self.record_public_power(art,"remove",Some(0),false,false);
                }
                return None;
            }
        }
        if !crate::powers::is_ported(id) {
            if self.fault.is_none() {
                self.fault_power = Some(id);
            }
            self.unported("unported power");
        }
        if existing.is_none() {
            amount = self.scale_power_for_multiplayer(id, amount, target);
        }
        // C# TemporaryStrengthPower.BeforeApplied: the underlying Strength is applied
        // before a fresh power is attached; subsequent stacks use AfterPowerAmountChanged.
        if existing.is_none() && (Self::is_temporary_strength_loss(id) || matches!(id, PowerId::ReptileTrinket | PowerId::Coordinate | PowerId::SetupStrike | PowerId::FlexPotion)) {
            let sign = if Self::is_temporary_strength_loss(id) { -1 } else { 1 };
            self.creature_apply_power(target, PowerId::Strength, sign * amount, applier, source, false, false);
        }
        // C# TemporaryDexterityPower.BeforeApplied: Anticipate/Fade/HelicalDart
        // apply the underlying Dexterity before the wrapper is attached.
        if existing.is_none() && matches!(id, PowerId::Anticipate | PowerId::Fade | PowerId::HelicalDart) {
            self.creature_apply_power(target, PowerId::Dexterity, amount, applier, source, false, false);
        }
        // TemporaryFocusPower.BeforeApplied attaches Focus before its wrapper.
        if existing.is_none() && Self::is_temporary_focus(id) {
            self.creature_apply_power(target, PowerId::Focus, Self::temporary_focus_sign(id)*amount, applier, source, false, false);
        }
        let before_amount=existing.map(|p|self.powers[p as usize].amount);
        let applied;
        let delta;
        if let Some(e) = existing {
            let pw = &mut self.powers[e as usize];
            if pw.applier.is_none() {
                pw.applier = applier;
            }
            if ignore_next {
                pw.ignore_next_instance = true;
            }
            pw.amount += amount;
            delta = amount;
            applied = Some(e);
        } else {
            // Nested BeforeApplied commands may attach powers first (e.g.
            // ReptileTrinket's Strength). Arena order is also listener/display
            // order, so finalize this reserved identity after those powers and
            // rebase the references retained by the before-amount hook.
            let p = if changing as usize + 1 == self.powers.len() {
                changing
            } else {
                let p = self.powers.len() as PRef;
                self.powers.push(self.powers[changing as usize].clone());
                for relic in &mut self.run.relics {
                    if let Some(refs) = &mut relic.doubled_powers {
                        for reference in refs {
                            if *reference == changing { *reference = p; }
                        }
                    }
                }
                p
            };
            self.powers[p as usize].amount = amount;
            let cr = &mut self.creatures[target as usize];
            let extra = cr.powers.iter().any(|&q| self.powers[q as usize].id == id);
            cr.attach_power(p, id, extra);
            delta = amount;
            applied = Some(p);
        }
        if let Some(p)=applied {
            self.record_public_power(p,if existing.is_none() {"apply"}else{"change"},before_amount,true,false);
        }
        if id == PowerId::Doom && existing.is_none()
            && let Some(p) = applier.filter(|&p| self.players.is_player(p))
        {
            self.hist.doom_appliers_this_turn[p as usize] = true;
        }
        // C# VoidFormPower.BeforeApplied / BeforePowerAmountChanged(this) ->
        // HideTemporaryZeroCostVisual: cardsPlayedThisTurn = 999999999, so no card is free
        // until the owner's next turn start resets it.
        if id == PowerId::VoidForm && let Some(p) = applied {
            self.powers[p as usize].vars[0] = 999_999_999;
        }
        // C# TankPower.AfterApplied: only a new Tank instance guards living
        // player teammates, excluding its owner; each Guarded retains its applier.
        if id == PowerId::Tank && existing.is_none() {
            for ally in self.living_players() {
                if ally != target {
                    self.apply_power_to(ally, PowerId::Guarded, amount, Some(target));
                }
            }
        }
        // C# CoveredPower.AfterApplied / InterceptPower.AddCoveredCreature:
        // one interceptor holds a distinct list of covered player creatures.
        if id == PowerId::Covered && existing.is_none() && let Some(a) = applier {
            let interceptor = self.find_power(a, PowerId::Intercept)
                .or_else(|| self.apply_power_to(a, PowerId::Intercept, 1, Some(target)));
            if let Some(p) = interceptor {
                self.powers[p as usize].vars[0] |= 1 << target;
            }
        }
        if delta != 0 {
            self.after_power_amount_changed(target, id, delta, applier, source);
        }
        if let Some(e) = existing
            && self.powers[e as usize].should_remove()
            && (self.cr(target).powers.contains(&e) || self.cr(target).extra.contains(&e))
        {
            self.remove_power(target, e);
        }
        applied
    }

    /// C# `PowerCmd.Apply` of a fresh instanced power whose `Target` is set first (one instance
    /// per player, e.g. GremlinMerc Thievery, Aeonglass Withering Presence).
    pub fn apply_targeted_power(&mut self, owner: CId, id: PowerId, amount: i32, applier: Option<CId>, target: CId) -> Option<PRef> {
        let p = self.creature_apply_power(owner, id, amount, applier, None, false, true)?;
        self.powers[p as usize].target = Some(target);
        Some(p)
    }

    /// C# `CombatState.Allies`: players in `Players` order, then pets.
    pub fn player_side(&self) -> ArrayVec<CId, { crate::player::MAX_PLAYERS + MAX_PET_ALLIES }> {
        (0..self.players.len() as CId).chain(self.allies.iter().copied()).collect()
    }

    /// `CombatState.apply_power_to`.
    pub fn apply_power_to(&mut self, target: CId, id: PowerId, amount: i32, applier: Option<CId>) -> Option<PRef> {
        self.apply_power_from(target, id, amount, applier, self.active_card)
    }

    /// PowerCmd.Apply's actual cardSource. Relic/power callbacks pass None even
    /// while a card is playing (e.g. Shuriken.cs:110, EnvenomPower.cs:24).
    pub(crate) fn apply_power_from(&mut self, target: CId, id: PowerId, amount: i32, applier: Option<CId>, source: Option<KRef>) -> Option<PRef> {
        if !self.can_receive_power(target) {
            return None;
        }
        // r7 CombatState.apply_power_to checks ShouldAllowHitting even when a dead
        // reviving creature remains in combat. Reattach/Adaptable/Illusion reject
        // follow-up power commands until their revive completes.
        let applier = applier.or_else(|| source.map(|k| self.cards[k as usize].owner))
            .or(if self.side == Side::Player { Some(PLAYER) } else { None });
        let existing = self.find_power(target, id);
        let applied = self.creature_apply_power(target, id, amount, applier, source, false, false);
        if self.cr(target).side == Side::Player && existing.is_none() {
            let p = applied.or_else(|| self.find_power(target, id));
            if let Some(p) = p && power_meta(self.powers[p as usize].id).power_type == PowerType::Debuff {
                self.powers[p as usize].skip_next_tick = true;
            }
        }
        applied
    }

    /// PowerCmd.Apply -> Creature.CanReceivePowers. HP zero alone does not reject powers;
    /// reviving monsters may reject them through ShouldAllowHitting.
    pub(crate) fn can_receive_power(&self, target: CId) -> bool {
        !self.is_over && !self.is_combat_ending_live()
            && self.cr(target).in_combat && !self.cr(target).escaped
            && self.powers_allow_hitting(target)
    }
}
