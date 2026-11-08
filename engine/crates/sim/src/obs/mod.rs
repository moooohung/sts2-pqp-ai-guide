//! Frozen r7 structured-v2 raw-observation wire contract for the CPU simulator.
//!
//! The caller owns all tensor memory. This module writes directly into those slices so the
//! observation path can run once per lane and decision without allocating.

mod encoder;
pub mod rule_admission;
mod tokens;
pub mod multiplayer;
pub mod mp_menu;

use crate::card::CId;

pub use encoder::{write_combat_obs_and_mask, write_combat_obs_and_mask_for_player};
pub use rule_admission::{AdmissionRow, admitted as rule_admitted, parsed_rows as rule_admission_rows};
#[cfg(feature = "selector-proof")]
pub use encoder::write_combat_proof_obs_and_mask;
pub use encoder::write_neow_obs;
pub use encoder::write_run_state;
pub use encoder::write_run_potion_candidate;
#[cfg(feature="selector-proof")]
pub use encoder::compare_card_semantics;
#[cfg(feature="selector-proof")]
pub use encoder::write_selector_proof_obs;
/// Vocabulary identity normalization: frozen r7 prefix plus explicit r8 additions.
pub use tokens::token_id as frozen_token_id;

// Frozen r7 `gym_env/action_space.py` + `core/constants.py` action geometry.
// Keep these independent of Rust collection capacities: they are policy ABI values.
pub const R7_MAX_HAND_SIZE: usize = 10;
pub const R7_MAX_ENEMIES: usize = 5;
pub const R7_MAX_POTION_SLOTS: usize = 9;
pub const R7_POTION_TARGET_OPTIONS: usize = 1 + R7_MAX_ENEMIES;
pub const R7_POTION_ACTION_START: usize = 1 + R7_MAX_HAND_SIZE + R7_MAX_HAND_SIZE * R7_MAX_ENEMIES;
pub const R7_ACTION_SPACE_SIZE: usize =
    R7_POTION_ACTION_START + R7_MAX_POTION_SLOTS * R7_POTION_TARGET_OPTIONS;

pub const STATE_CAPACITY: usize = 512;
pub const CANDIDATE_CAPACITY: usize = 256;
pub const TOKEN_SLOTS: usize = 6;
pub const SCALAR_SLOTS: usize = 8;
pub const BOOLEAN_FEATURES: usize = 16;
pub const ROUTE_BYTES_PER_CANDIDATE: usize = STATE_CAPACITY / 8;
pub const SOURCE_LINK_BYTES: usize = CANDIDATE_CAPACITY * ROUTE_BYTES_PER_CANDIDATE;
pub const BOSS_FEATURES: usize = 6;

const STATE_TOKEN_LEN: usize = STATE_CAPACITY * TOKEN_SLOTS;
const STATE_SCALAR_LEN: usize = STATE_CAPACITY * SCALAR_SLOTS;
const STATE_TAIL_LEN: usize = STATE_CAPACITY * BOOLEAN_FEATURES;
const CANDIDATE_TOKEN_LEN: usize = CANDIDATE_CAPACITY * TOKEN_SLOTS;
const CANDIDATE_SCALAR_LEN: usize = CANDIDATE_CAPACITY * SCALAR_SLOTS;
const CANDIDATE_TAIL_LEN: usize = CANDIDATE_CAPACITY * BOOLEAN_FEATURES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObsError {
    BufferTooSmall {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    TooManyStateRows,
    TooManyCandidates,
    UnknownToken {
        namespace: &'static str,
    },
    InvalidActionSlot,
    UnsupportedState(&'static str),
}

impl core::fmt::Display for ObsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferTooSmall {
                field,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "observation buffer {field} has {actual} elements; expected at least {expected}"
                )
            }
            Self::TooManyStateRows => {
                write!(f, "r7 observation exceeds {STATE_CAPACITY} state rows")
            }
            Self::TooManyCandidates => {
                write!(f, "r7 observation exceeds {CANDIDATE_CAPACITY} candidates")
            }
            Self::UnknownToken { namespace } => {
                write!(f, "unrepresentable r7 token in namespace {namespace}")
            }
            Self::InvalidActionSlot => write!(f, "candidate action slot is out of range"),
            Self::UnsupportedState(reason) => write!(
                f,
                "observation state cannot be represented exactly: {reason}"
            ),
        }
    }
}

impl std::error::Error for ObsError {}

/// The additional run-layer facts not owned by `Combat` yet.
///
/// `public_act1_boss` is the exact six-float public boss feature from r7. The choice prompt is
/// kept here because the current combat engine's `PendingChoice` does not retain r7's prompt.
pub struct CombatObservationContext<'a> {
    pub public_act1_boss: [f32; BOSS_FEATURES],
    pub choice_prompt: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Empty,
    EndTurn,
    ConfirmChoice,
    PlayCard,
    UsePotion,
    Choose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSlot {
    /// Acting player retained alongside the hand/target binding (legacy decoder uses 0).
    pub actor: CId,
    pub kind: ActionKind,
    /// The original r7 discrete simulator action, before candidate-slot reordering.
    pub simulator_action: u16,
    pub hand_index: u8,
    pub target: Option<CId>,
    pub potion_slot: u8,
    pub choice_index: u8,
}

impl ActionSlot {
    const EMPTY: Self = Self {
        actor: 0,
        kind: ActionKind::Empty,
        simulator_action: 0,
        hand_index: 0,
        target: None,
        potion_slot: 0,
        choice_index: 0,
    };
}

/// Reused action mapping for one observation. Slot indices here are policy candidate slots (0..255),
/// not r7's underlying discrete action indices.
pub struct ActionSlots {
    slots: [ActionSlot; CANDIDATE_CAPACITY],
    len: usize,
}

impl Default for ActionSlots {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionSlots {
    pub const fn new() -> Self {
        Self {
            slots: [ActionSlot::EMPTY; CANDIDATE_CAPACITY],
            len: 0,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.slots.fill(ActionSlot::EMPTY);
        self.len = 0;
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, slot: usize) -> Option<ActionSlot> {
        (slot < self.len).then_some(self.slots[slot])
    }

    #[inline]
    pub(crate) fn push(&mut self, action: ActionSlot) -> Result<usize, ObsError> {
        if self.len == CANDIDATE_CAPACITY {
            return Err(ObsError::TooManyCandidates);
        }
        let slot = self.len;
        self.slots[slot] = action;
        self.len += 1;
        Ok(slot)
    }

    /// Append the exact decoded binding for an r7 discrete action. `enemy_ids` must be the
    /// combat's enemy array in r7 order; target offsets in the action ABI refer to that order.
    #[inline]
    pub fn push_simulator_action(
        &mut self,
        simulator_action: u16,
        enemy_ids: &[CId],
        choice_mode: bool,
    ) -> Result<usize, ObsError> {
        let action = decode_simulator_action(simulator_action, enemy_ids, choice_mode)?;
        self.push(action)
    }
}

/// Decode r7's fixed action index into the run/combat binding represented by an action slot.
/// This only decodes the index; callers must separately establish that the action is legal.
///
/// Source: frozen r7 `gym_env/action_space.py` (`action_to_card_and_target` and
/// `action_to_potion_and_target`) and `core/constants.py` (action-space constants).
pub fn decode_simulator_action(
    simulator_action: u16,
    enemy_ids: &[CId],
    choice_mode: bool,
) -> Result<ActionSlot, ObsError> {
    let action = simulator_action as usize;
    if choice_mode {
        return if action == 0 {
            Ok(ActionSlot {
                kind: ActionKind::ConfirmChoice,
                simulator_action,
                ..ActionSlot::EMPTY
            })
        } else if action < R7_ACTION_SPACE_SIZE {
            Ok(ActionSlot {
                kind: ActionKind::Choose,
                simulator_action,
                choice_index: u8::try_from(action - 1).map_err(|_| ObsError::InvalidActionSlot)?,
                ..ActionSlot::EMPTY
            })
        } else {
            Err(ObsError::InvalidActionSlot)
        };
    }

    if action == 0 {
        return Ok(ActionSlot {
            kind: ActionKind::EndTurn,
            simulator_action,
            ..ActionSlot::EMPTY
        });
    }
    if action < R7_POTION_ACTION_START {
        if action <= R7_MAX_HAND_SIZE {
            return Ok(ActionSlot {
                kind: ActionKind::PlayCard,
                simulator_action,
                hand_index: (action - 1) as u8,
                ..ActionSlot::EMPTY
            });
        }
        let adjusted = action - 1 - R7_MAX_HAND_SIZE;
        let hand_index = adjusted / R7_MAX_ENEMIES;
        let target_index = adjusted % R7_MAX_ENEMIES;
        let target = *enemy_ids
            .get(target_index)
            .ok_or(ObsError::InvalidActionSlot)?;
        return Ok(ActionSlot {
            kind: ActionKind::PlayCard,
            simulator_action,
            hand_index: u8::try_from(hand_index).map_err(|_| ObsError::InvalidActionSlot)?,
            target: Some(target),
            ..ActionSlot::EMPTY
        });
    }

    if action >= R7_ACTION_SPACE_SIZE {
        return Err(ObsError::InvalidActionSlot);
    }
    let adjusted = action - R7_POTION_ACTION_START;
    let potion_slot = adjusted / R7_POTION_TARGET_OPTIONS;
    if potion_slot >= R7_MAX_POTION_SLOTS {
        return Err(ObsError::InvalidActionSlot);
    }
    let target_offset = adjusted % R7_POTION_TARGET_OPTIONS;
    let target = if target_offset == 0 {
        None
    } else {
        Some(
            *enemy_ids
                .get(target_offset - 1)
                .ok_or(ObsError::InvalidActionSlot)?,
        )
    };
    Ok(ActionSlot {
        kind: ActionKind::UsePotion,
        simulator_action,
        target,
        potion_slot: potion_slot as u8,
        ..ActionSlot::EMPTY
    })
}

/// Decode targeted slots using the hand card's target side, preserving the r7 action layout.
/// C# CardModel.IsValidTarget allows living same-side players for AnyAlly, including self.
pub fn decode_combat_action(
    simulator_action: u16,
    combat: &crate::combat::Combat,
    enemy_ids: &[CId],
    choice_mode: bool,
) -> Result<ActionSlot, ObsError> {
    decode_combat_action_for_player(simulator_action, combat, crate::combat::PLAYER, enemy_ids, choice_mode)
}

/// Multiplayer counterpart; the caller supplies the player whose decision is being encoded.
pub fn decode_combat_action_for_player(
    simulator_action: u16,
    combat: &crate::combat::Combat,
    actor: CId,
    enemy_ids: &[CId],
    choice_mode: bool,
) -> Result<ActionSlot, ObsError> {
    if !combat.players.is_player(actor) {
        return Err(ObsError::InvalidActionSlot);
    }
    if combat.pending_choice.is_some() && combat.pending_choice_for_player(actor).is_none() {
        return Err(ObsError::InvalidActionSlot);
    }
    let action = simulator_action as usize;
    if !choice_mode && (R7_POTION_ACTION_START..R7_ACTION_SPACE_SIZE).contains(&action) {
        let adjusted = action - R7_POTION_ACTION_START;
        let potion_slot = adjusted / R7_POTION_TARGET_OPTIONS;
        let target_offset = adjusted % R7_POTION_TARGET_OPTIONS;
        if let Some(Some(id)) = combat.potion_slots_for_player(actor)
            .and_then(|slots| slots.get(potion_slot))
            && crate::tables::POTION_INFO[id.index()].target == crate::tables::PotionTargetType::AnyPlayer
        {
            let seat = if target_offset == 0 { usize::from(actor) } else { target_offset - 1 };
            if target_offset != 0 && seat == usize::from(actor) {
                return Err(ObsError::InvalidActionSlot);
            }
            if seat >= combat.players.len() || !combat.cr(seat as CId).is_alive() {
                return Err(ObsError::InvalidActionSlot);
            }
            return Ok(ActionSlot {
                kind: ActionKind::UsePotion,
                simulator_action,
                potion_slot: potion_slot as u8,
                target: Some(seat as CId),
                actor,
                ..ActionSlot::EMPTY
            });
        }
    }
    if !choice_mode && action > R7_MAX_HAND_SIZE && action < R7_POTION_ACTION_START {
        let adjusted = action - 1 - R7_MAX_HAND_SIZE;
        let hand_index = adjusted / R7_MAX_ENEMIES;
        let target = adjusted % R7_MAX_ENEMIES;
        if let Some(&k) = combat.players[actor].hand.get(hand_index)
            && combat.card_target_type(k) == crate::tables::TargetType::AnyAlly
        {
            if target >= combat.players.len() || !combat.cr(target as CId).is_alive() {
                return Err(ObsError::InvalidActionSlot);
            }
            return Ok(ActionSlot {
                kind: ActionKind::PlayCard,
                simulator_action,
                hand_index: hand_index as u8,
                target: Some(target as CId),
                actor,
                ..ActionSlot::EMPTY
            });
        }
    }
    let mut binding = decode_simulator_action(simulator_action, enemy_ids, choice_mode)?;
    binding.actor = actor;
    Ok(binding)
}

/// Typed views over the exact raw r7 tensor layout. Construction validates every slice once;
/// subsequent row writes are bounds-checked against the fixed contract capacities.
pub struct ObsBuffers<'a> {
    pub state_kind_ids: &'a mut [u16],
    pub state_token_ids: &'a mut [u16],
    pub state_numeric_values: &'a mut [i32],
    pub state_numeric_scales: &'a mut [f32],
    pub state_numeric_tail: &'a mut [f32],
    pub state_mask: &'a mut [i8],
    pub candidate_kind_ids: &'a mut [u16],
    pub candidate_token_ids: &'a mut [u16],
    pub candidate_numeric_values: &'a mut [i32],
    pub candidate_numeric_scales: &'a mut [f32],
    pub candidate_numeric_tail: &'a mut [f32],
    pub choice_purpose_ids: &'a mut [u16],
    pub model_mask: &'a mut [i8],
    pub source_applicable: &'a mut [i8],
    pub target_applicable: &'a mut [i8],
    pub source_known: &'a mut [i8],
    pub target_known: &'a mut [i8],
    pub source_links_bits: &'a mut [u8],
    pub target_links_bits: &'a mut [u8],
    pub legal_mask: &'a mut [bool],
    pub public_act1_boss: &'a mut [f32],
    state_rows: usize,
    candidate_rows: usize,
}

impl<'a> ObsBuffers<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        state_kind_ids: &'a mut [u16],
        state_token_ids: &'a mut [u16],
        state_numeric_values: &'a mut [i32],
        state_numeric_scales: &'a mut [f32],
        state_numeric_tail: &'a mut [f32],
        state_mask: &'a mut [i8],
        candidate_kind_ids: &'a mut [u16],
        candidate_token_ids: &'a mut [u16],
        candidate_numeric_values: &'a mut [i32],
        candidate_numeric_scales: &'a mut [f32],
        candidate_numeric_tail: &'a mut [f32],
        choice_purpose_ids: &'a mut [u16],
        model_mask: &'a mut [i8],
        source_applicable: &'a mut [i8],
        target_applicable: &'a mut [i8],
        source_known: &'a mut [i8],
        target_known: &'a mut [i8],
        source_links_bits: &'a mut [u8],
        target_links_bits: &'a mut [u8],
        legal_mask: &'a mut [bool],
        public_act1_boss: &'a mut [f32],
    ) -> Result<Self, ObsError> {
        check_len("state_kind_ids", state_kind_ids.len(), STATE_CAPACITY)?;
        check_len("state_token_ids", state_token_ids.len(), STATE_TOKEN_LEN)?;
        check_len(
            "state_numeric_values",
            state_numeric_values.len(),
            STATE_SCALAR_LEN,
        )?;
        check_len(
            "state_numeric_scales",
            state_numeric_scales.len(),
            STATE_SCALAR_LEN,
        )?;
        check_len(
            "state_numeric_tail",
            state_numeric_tail.len(),
            STATE_TAIL_LEN,
        )?;
        check_len("state_mask", state_mask.len(), STATE_CAPACITY)?;
        check_len(
            "candidate_kind_ids",
            candidate_kind_ids.len(),
            CANDIDATE_CAPACITY,
        )?;
        check_len(
            "candidate_token_ids",
            candidate_token_ids.len(),
            CANDIDATE_TOKEN_LEN,
        )?;
        check_len(
            "candidate_numeric_values",
            candidate_numeric_values.len(),
            CANDIDATE_SCALAR_LEN,
        )?;
        check_len(
            "candidate_numeric_scales",
            candidate_numeric_scales.len(),
            CANDIDATE_SCALAR_LEN,
        )?;
        check_len(
            "candidate_numeric_tail",
            candidate_numeric_tail.len(),
            CANDIDATE_TAIL_LEN,
        )?;
        check_len(
            "choice_purpose_ids",
            choice_purpose_ids.len(),
            CANDIDATE_CAPACITY,
        )?;
        check_len("model_mask", model_mask.len(), CANDIDATE_CAPACITY)?;
        check_len(
            "source_applicable",
            source_applicable.len(),
            CANDIDATE_CAPACITY,
        )?;
        check_len(
            "target_applicable",
            target_applicable.len(),
            CANDIDATE_CAPACITY,
        )?;
        check_len("source_known", source_known.len(), CANDIDATE_CAPACITY)?;
        check_len("target_known", target_known.len(), CANDIDATE_CAPACITY)?;
        check_len(
            "source_links_bits",
            source_links_bits.len(),
            SOURCE_LINK_BYTES,
        )?;
        check_len(
            "target_links_bits",
            target_links_bits.len(),
            SOURCE_LINK_BYTES,
        )?;
        check_len("legal_mask", legal_mask.len(), CANDIDATE_CAPACITY)?;
        check_len("public_act1_boss", public_act1_boss.len(), BOSS_FEATURES)?;
        Ok(Self {
            state_kind_ids,
            state_token_ids,
            state_numeric_values,
            state_numeric_scales,
            state_numeric_tail,
            state_mask,
            candidate_kind_ids,
            candidate_token_ids,
            candidate_numeric_values,
            candidate_numeric_scales,
            candidate_numeric_tail,
            choice_purpose_ids,
            model_mask,
            source_applicable,
            target_applicable,
            source_known,
            target_known,
            source_links_bits,
            target_links_bits,
            legal_mask,
            public_act1_boss,
            state_rows: 0,
            candidate_rows: 0,
        })
    }

    #[inline]
    pub fn clear(&mut self, context: &CombatObservationContext<'_>) -> Result<(), ObsError> {
        let candidate_kind = tokens::kind_id("candidate").ok_or(ObsError::UnknownToken {
            namespace: "entity_kind",
        })?;
        self.state_kind_ids.fill(0);
        self.state_token_ids[..STATE_TOKEN_LEN].fill(0);
        self.state_numeric_values[..STATE_SCALAR_LEN].fill(0);
        self.state_numeric_scales[..STATE_SCALAR_LEN].fill(0.0);
        self.state_numeric_tail[..STATE_TAIL_LEN].fill(0.0);
        self.state_mask.fill(0);
        self.candidate_kind_ids[..CANDIDATE_CAPACITY].fill(candidate_kind);
        self.candidate_token_ids[..CANDIDATE_TOKEN_LEN].fill(0);
        self.candidate_numeric_values[..CANDIDATE_SCALAR_LEN].fill(0);
        self.candidate_numeric_scales[..CANDIDATE_SCALAR_LEN].fill(0.0);
        self.candidate_numeric_tail[..CANDIDATE_TAIL_LEN].fill(0.0);
        self.choice_purpose_ids.fill(0);
        self.model_mask.fill(0);
        self.source_applicable.fill(0);
        self.target_applicable.fill(0);
        self.source_known.fill(1);
        self.target_known.fill(1);
        self.source_links_bits[..SOURCE_LINK_BYTES].fill(0);
        self.target_links_bits[..SOURCE_LINK_BYTES].fill(0);
        self.legal_mask.fill(false);
        self.public_act1_boss[..BOSS_FEATURES].copy_from_slice(&context.public_act1_boss);
        self.state_rows = 0;
        self.candidate_rows = 0;
        Ok(())
    }

    #[inline]
    pub fn push_state(&mut self, kind: &'static str) -> Result<usize, ObsError> {
        if self.state_rows == STATE_CAPACITY {
            return Err(ObsError::TooManyStateRows);
        }
        let kind_id = tokens::kind_id(kind).ok_or(ObsError::UnknownToken {
            namespace: "entity_kind",
        })?;
        let row = self.state_rows;
        self.state_kind_ids[row] = kind_id;
        self.state_mask[row] = 1;
        self.state_rows += 1;
        Ok(row)
    }

    #[inline]
    pub fn push_candidate(&mut self) -> Result<usize, ObsError> {
        if self.candidate_rows == CANDIDATE_CAPACITY {
            return Err(ObsError::TooManyCandidates);
        }
        let row = self.candidate_rows;
        self.legal_mask[row] = true;
        self.candidate_rows += 1;
        Ok(row)
    }

    #[inline]
    pub fn state_token(&mut self, row: usize, slot: usize, id: u16) {
        self.state_token_ids[row * TOKEN_SLOTS + slot] = id;
    }

    #[inline]
    pub fn candidate_token(&mut self, row: usize, slot: usize, id: u16) {
        self.candidate_token_ids[row * TOKEN_SLOTS + slot] = id;
    }

    #[inline]
    pub fn state_token_named(
        &mut self,
        row: usize,
        slot: usize,
        namespace: &'static str,
        value: &str,
    ) -> Result<(), ObsError> {
        self.state_token(row, slot, token(namespace, value)?);
        Ok(())
    }

    #[inline]
    pub fn candidate_token_named(
        &mut self,
        row: usize,
        slot: usize,
        namespace: &'static str,
        value: &str,
    ) -> Result<(), ObsError> {
        self.candidate_token(row, slot, token(namespace, value)?);
        Ok(())
    }

    #[inline]
    pub fn state_scalar(&mut self, row: usize, slot: usize, value: i32, scale: f32, known: bool) {
        let index = row * SCALAR_SLOTS + slot;
        if known {
            self.state_numeric_values[index] = value;
            self.state_numeric_scales[index] = scale;
        }
    }

    #[inline]
    pub fn candidate_scalar(
        &mut self,
        row: usize,
        slot: usize,
        value: i32,
        scale: f32,
        known: bool,
    ) {
        let index = row * SCALAR_SLOTS + slot;
        if known {
            self.candidate_numeric_values[index] = value;
            self.candidate_numeric_scales[index] = scale;
        }
    }

    #[inline]
    pub fn state_tail_value(&mut self, row: usize, slot: usize, value: f32) {
        self.state_numeric_tail[row * BOOLEAN_FEATURES + slot] = value;
    }

    #[inline]
    pub fn candidate_tail_value(&mut self, row: usize, slot: usize, value: f32) {
        self.candidate_numeric_tail[row * BOOLEAN_FEATURES + slot] = value;
    }

    #[inline]
    pub fn state_bool(&mut self, row: usize, slot: usize, value: bool) {
        self.state_tail_value(row, slot, if value { 1.0 } else { 0.0 });
    }

    #[inline]
    pub fn candidate_bool(&mut self, row: usize, slot: usize, value: bool) {
        self.candidate_tail_value(row, slot, if value { 1.0 } else { 0.0 });
    }

    #[inline]
    pub fn set_model_eligible(&mut self, slot: usize, eligible: bool) {
        self.model_mask[slot] = i8::from(eligible);
    }

    #[inline]
    pub fn set_source_applicable(&mut self, slot: usize, applicable: bool, known: bool) {
        self.source_applicable[slot] = i8::from(applicable);
        self.source_known[slot] = i8::from(known);
    }

    #[inline]
    pub fn set_target_applicable(&mut self, slot: usize, applicable: bool, known: bool) {
        self.target_applicable[slot] = i8::from(applicable);
        self.target_known[slot] = i8::from(known);
    }

    #[inline]
    pub fn set_choice_purpose(&mut self, slot: usize, purpose: &str) -> Result<(), ObsError> {
        self.choice_purpose_ids[slot] =
            tokens::choice_purpose_id(purpose).ok_or(ObsError::UnknownToken {
                namespace: "choice_purpose",
            })?;
        Ok(())
    }

    #[inline]
    pub fn source_link(&mut self, slot: usize, state_row: usize) {
        set_link_bit(self.source_links_bits, slot, state_row);
    }

    #[inline]
    pub fn target_link(&mut self, slot: usize, state_row: usize) {
        set_link_bit(self.target_links_bits, slot, state_row);
    }

    #[inline]
    pub fn state_rows(&self) -> usize {
        self.state_rows
    }

    #[inline]
    pub fn candidate_rows(&self) -> usize {
        self.candidate_rows
    }
}

#[inline]
fn check_len(field: &'static str, actual: usize, expected: usize) -> Result<(), ObsError> {
    if actual < expected {
        Err(ObsError::BufferTooSmall {
            field,
            expected,
            actual,
        })
    } else {
        Ok(())
    }
}

#[inline]
fn set_link_bit(bits: &mut [u8], candidate: usize, state_row: usize) {
    let byte = candidate * ROUTE_BYTES_PER_CANDIDATE + state_row / 8;
    bits[byte] |= 1 << (state_row % 8);
}

#[inline]
fn token(namespace: &'static str, value: &str) -> Result<u16, ObsError> {
    tokens::token_id(namespace, value).ok_or(ObsError::UnknownToken { namespace })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_wire_dimensions_and_defaults_match_r7() {
        let mut state_kind = [0u16; STATE_CAPACITY];
        let mut state_tokens = [0u16; STATE_TOKEN_LEN];
        let mut state_values = [0i32; STATE_SCALAR_LEN];
        let mut state_scales = [0f32; STATE_SCALAR_LEN];
        let mut state_tail = [0f32; STATE_TAIL_LEN];
        let mut state_mask = [0i8; STATE_CAPACITY];
        let mut candidate_kind = [0u16; CANDIDATE_CAPACITY];
        let mut candidate_tokens = [0u16; CANDIDATE_TOKEN_LEN];
        let mut candidate_values = [0i32; CANDIDATE_SCALAR_LEN];
        let mut candidate_scales = [0f32; CANDIDATE_SCALAR_LEN];
        let mut candidate_tail = [0f32; CANDIDATE_TAIL_LEN];
        let mut choice_purpose = [0u16; CANDIDATE_CAPACITY];
        let mut model_mask = [0i8; CANDIDATE_CAPACITY];
        let mut source_applicable = [0i8; CANDIDATE_CAPACITY];
        let mut target_applicable = [0i8; CANDIDATE_CAPACITY];
        let mut source_known = [0i8; CANDIDATE_CAPACITY];
        let mut target_known = [0i8; CANDIDATE_CAPACITY];
        let mut source_links = [0u8; SOURCE_LINK_BYTES];
        let mut target_links = [0u8; SOURCE_LINK_BYTES];
        let mut legal_mask = [false; CANDIDATE_CAPACITY];
        let mut boss = [0f32; BOSS_FEATURES];
        let ctx = CombatObservationContext {
            public_act1_boss: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            choice_prompt: "",
        };
        let mut b = ObsBuffers::new(
            &mut state_kind,
            &mut state_tokens,
            &mut state_values,
            &mut state_scales,
            &mut state_tail,
            &mut state_mask,
            &mut candidate_kind,
            &mut candidate_tokens,
            &mut candidate_values,
            &mut candidate_scales,
            &mut candidate_tail,
            &mut choice_purpose,
            &mut model_mask,
            &mut source_applicable,
            &mut target_applicable,
            &mut source_known,
            &mut target_known,
            &mut source_links,
            &mut target_links,
            &mut legal_mask,
            &mut boss,
        )
        .unwrap();
        b.clear(&ctx).unwrap();
        assert_eq!(b.state_rows(), 0);
        assert_eq!(b.candidate_rows(), 0);

        let global = b.push_state("global").unwrap();
        b.state_token_named(global, 0, "phase", "COMBAT").unwrap();
        b.state_scalar(global, 0, 64, 100.0, true);
        b.state_bool(global, 0, true);
        assert_eq!(b.state_rows(), 1);
        let candidate = b.push_candidate().unwrap();
        b.candidate_token_named(candidate,0,"action_kind","pick_special_card").unwrap();
        drop(b);
        assert_eq!(candidate_tokens[0],sts2_wire::vocab::PICK_SPECIAL_CARD);
        assert_eq!(state_kind[0], 2);
        assert_eq!(state_tokens[0], 2);
        assert_eq!(state_values[0], 64);
        assert_eq!(state_scales[0], 100.0);
        assert_eq!(state_tail[0], 1.0);
        assert_eq!(candidate_kind[0], tokens::kind_id("candidate").unwrap());
        assert_eq!(source_known[0], 1);
        assert_eq!(target_known[255], 1);
        assert_eq!(boss, ctx.public_act1_boss);
    }

    #[test]
    fn packed_routes_use_little_endian_row_bits() {
        let mut links = [0u8; SOURCE_LINK_BYTES];
        set_link_bit(&mut links, 3, 0);
        set_link_bit(&mut links, 3, 7);
        set_link_bit(&mut links, 3, 8);
        set_link_bit(&mut links, 4, 8);
        let base = 3 * ROUTE_BYTES_PER_CANDIDATE;
        assert_eq!(links[base], 0b1000_0001);
        assert_eq!(links[base + 1], 0b0000_0001);
        assert_eq!(links[4 * ROUTE_BYTES_PER_CANDIDATE + 1], 0b0000_0001);
    }

    #[test]
    fn special_card_pick_is_an_append_only_r8_v1_extension() {
        assert_eq!(token("action_kind", "pick_special_card").unwrap() as usize,
            sts2_wire::vocab::R7_TOKEN_VOCAB);
        assert_eq!(token("action_kind", "pick_card").unwrap() as usize,
            sts2_wire::vocab::PICK_CARD);
    }

    #[test]
    fn r7_action_indices_decode_to_exact_bindings() {
        let enemies = [4, 7, 9, 11, 12];
        let end_turn = decode_simulator_action(0, &enemies, false).unwrap();
        assert_eq!(end_turn.kind, ActionKind::EndTurn);
        let self_card = decode_simulator_action(3, &enemies, false).unwrap();
        assert_eq!(
            (self_card.kind, self_card.hand_index, self_card.target),
            (ActionKind::PlayCard, 2, None)
        );
        let targeted_card = decode_simulator_action(
            (1 + R7_MAX_HAND_SIZE + 2 * R7_MAX_ENEMIES + 3) as u16,
            &enemies,
            false,
        )
        .unwrap();
        assert_eq!(
            (targeted_card.hand_index, targeted_card.target),
            (2, Some(11))
        );
        let potion = decode_simulator_action(
            (R7_POTION_ACTION_START + 2 * R7_POTION_TARGET_OPTIONS + 2) as u16,
            &enemies,
            false,
        )
        .unwrap();
        assert_eq!(
            (potion.kind, potion.potion_slot, potion.target),
            (ActionKind::UsePotion, 2, Some(7))
        );
        let confirm = decode_simulator_action(0, &enemies, true).unwrap();
        assert_eq!(confirm.kind, ActionKind::ConfirmChoice);
        let choice = decode_simulator_action(5, &enemies, true).unwrap();
        assert_eq!((choice.kind, choice.choice_index), (ActionKind::Choose, 4));
    }

    #[test]
    fn r7_action_decoder_rejects_out_of_range_enemy_and_action() {
        assert_eq!(
            decode_simulator_action((1 + R7_MAX_HAND_SIZE + 4) as u16, &[2], false),
            Err(ObsError::InvalidActionSlot)
        );
        assert_eq!(
            decode_simulator_action(R7_ACTION_SPACE_SIZE as u16, &[], false),
            Err(ObsError::InvalidActionSlot)
        );
    }
}
