//! Per-player combat state (C# `Entities/Players/PlayerCombatState.cs`).
//!
//! Every player creature owns one `PlayerCombatState`; player creatures take creature ids
//! `0..players.len()`, so `players[owner]` finds the owner's state without a lookup. Solo play
//! is one entry. Card, power and relic code reaches the state through the model's owner
//! (`card.owner`, `power.owner`), never through a fixed player (docs/cpu_sim_design.md
//! "Multiplayer readiness").

use crate::card::{CId, Card, KRef};
use crate::combat::MAX_HAND_SIZE;
use arrayvec::ArrayVec;

/// Most players the game allows in one run.
pub const MAX_PLAYERS: usize = 4;

#[derive(Clone, Debug, Default)]
pub struct PlayerCombatState {
    /// The player's creature id (equal to its index in `Players`).
    pub creature: CId,
    /// C# Player.IsActiveForHooks; deactivated after death hooks and Osty's death.
    pub hooks_active: bool,
    /// Explicit pool identity for character-dependent effects; unset seats fail closed.
    pub character: Option<crate::character::Character>,
    /// C# PlayerCombatState.TurnNumber, including extra player turns.
    pub turn_number: i32,
    pub hand: ArrayVec<KRef, MAX_HAND_SIZE>,
    pub draw: Vec<KRef>,
    pub discard: Vec<KRef>,
    pub exhaust: Vec<KRef>,
    pub play: Vec<KRef>,
    /// C# PlayerCombatState.Pets, distinct from CombatState.Allies' arena.
    pub pets: Vec<CId>,
    /// Arena refs of the deck cards cloned into this combat, in deck order.
    pub starting_deck: Vec<KRef>,
    /// Persistent deck for additional players; player 0 uses `RunCtx::deck` directly.
    pub persistent_deck: Vec<Card>,
    /// Additional owners' persistent PlayerRng.Rewards; absent recordings fail closed.
    pub rewards_rng: Option<crate::rng::Rng>,
    /// Original deck cards held by Swipe instances, indexed by their per-instance vars[0].
    pub stolen_deck_cards: Vec<Option<Card>>,
    /// Additional seats' persistent gold. None means the caller did not supply it;
    /// player zero continues to use RunCtx::gold.
    pub gold: Option<i32>,
    /// Additional seats' persistent potion slots; None means not supplied.
    /// Player zero uses RunCtx::potions and max_potion_slots.
    pub potions: Option<Vec<Option<crate::tables::PotionId>>>,
    /// C# HeistPower.BeforeDeath adds a stolen-back reward to its Target player.
    pub extra_gold_reward: i32,
    pub energy: i32,
    pub stars: i32,
    pub base_max_energy: i32,
    pub character_pool: Option<crate::character::CardPool>,
    pub orbs: crate::orbs::OrbQueue,
}

impl PlayerCombatState {
    pub fn new(creature: CId, base_max_energy: i32, deck_len: usize) -> PlayerCombatState {
        PlayerCombatState {
            creature,
            hooks_active: true,
            character: None,
            turn_number: 0,
            hand: ArrayVec::new(),
            draw: Vec::with_capacity(48),
            discard: Vec::with_capacity(48),
            exhaust: Vec::with_capacity(16),
            play: Vec::with_capacity(4),
            pets: Vec::new(),
            starting_deck: Vec::with_capacity(deck_len),
            persistent_deck: Vec::new(),
            rewards_rng: None,
            stolen_deck_cards: Vec::new(),
            gold: None,
            potions: None,
            extra_gold_reward: 0,
            energy: 0,
            stars: 0,
            base_max_energy,
            character_pool: None,
            orbs: Default::default(),
        }
    }
}

/// `CombatState.Players`' combat states, indexed by the player's creature id.
#[derive(Clone, Debug, Default)]
pub struct Players(pub ArrayVec<PlayerCombatState, MAX_PLAYERS>);

impl Players {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `c` is a player creature with a combat state.
    #[inline]
    pub fn is_player(&self, c: CId) -> bool {
        (c as usize) < self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, PlayerCombatState> {
        self.0.iter()
    }
}

impl std::ops::Index<CId> for Players {
    type Output = PlayerCombatState;
    #[inline]
    fn index(&self, owner: CId) -> &PlayerCombatState {
        &self.0[owner as usize]
    }
}

impl std::ops::IndexMut<CId> for Players {
    #[inline]
    fn index_mut(&mut self, owner: CId) -> &mut PlayerCombatState {
        &mut self.0[owner as usize]
    }
}
