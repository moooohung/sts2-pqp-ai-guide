//! Frozen-r7 policy view, distinct from the richer multiplayer debug encoder.
//! See docs/mp_observation.md for source citations and omitted information.
use super::*;
use crate::{combat::Combat, run::{RunManager, RunPhase}};

/// Pending selectors take priority (including live/parked selectors). Otherwise
/// choose a living participating seat. `after` rotates ties in stable seat order.
/// None means terminal or automatic work, NOT an end-turn instruction.
pub fn combat_decision_owner(combat: &Combat, after: Option<CId>) -> Option<CId> {
    if combat.is_over || combat.fault.is_some() { return None; }
    let n = combat.players.len();
    let start = after.map_or(0, |seat| (usize::from(seat) + 1) % n);
    let seats = || (0..n).map(|offset| ((start + offset) % n) as CId);
    if let Some(owner) = seats().find(|&seat| combat.pending_choice_for_player(seat).is_some()) {
        return Some(owner);
    }
    if combat.side != crate::creature::Side::Player || combat.is_ending { return None; }
    seats().find(|&seat| combat.cr(seat).is_alive() && combat.is_player_taking_turn(seat))
}

/// Run menus currently belong to seat zero; multiplayer combat routes as above.
pub fn run_decision_owner(run: &RunManager, after: Option<CId>) -> Option<CId> {
    if run.phase() == RunPhase::Combat {
        run.combat().and_then(|combat| combat_decision_owner(combat, after))
    } else if run.clone().available_actions().is_empty() {
        None
    } else { Some(0) }
}

/// Write the existing r7 raw wire and action bindings for one seat without
/// mutating the simulation, activating its choices, or advancing any RNG.
/// Bindings retain absolute simulator actor/target IDs; do not rebase actions.
pub fn write_combat_perspective(
    combat: &Combat, actor: CId, context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>, slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    write_combat_perspective_mode(combat,actor,context,buffers,slots,false)
}

/// Offline original-DLL planner only. All production admission checks remain
/// enabled in write_combat_perspective.
#[cfg(feature="selector-proof")]
pub fn write_combat_proof_perspective(
    combat:&Combat,actor:CId,context:&CombatObservationContext<'_>,
    buffers:&mut ObsBuffers<'_>,slots:&mut ActionSlots,
) -> Result<(),ObsError> {
    write_combat_perspective_mode(combat,actor,context,buffers,slots,true)
}

fn write_combat_perspective_mode(
    combat:&Combat,actor:CId,context:&CombatObservationContext<'_>,
    buffers:&mut ObsBuffers<'_>,slots:&mut ActionSlots,proof:bool,
) -> Result<(),ObsError> {
    if !combat.players.is_player(actor) { return Err(ObsError::InvalidActionSlot); }
    if combat.fault.is_some() { return Err(ObsError::UnsupportedState("faulted combat")); }
    if combat.players.len() > 1 && combat.pending_choice_for_player(actor).is_none()
        && (combat.side != crate::creature::Side::Player
            || !combat.cr(actor).is_alive() || !combat.is_player_taking_turn(actor)) {
        return Err(ObsError::UnsupportedState("seat has no decision"));
    }
    let mut view = combat.clone();
    super::encoder::write_combat_obs_and_mask_for_player_inner(
        &mut view,actor,context,buffers,slots,proof)?;
    if combat.players.len() == 1 { return Ok(()); } // exact legacy solo bytes

    // structured_encoder.py:738-743 has only ascension, act, turnNumber.
    buffers.state_scalar(1, 2, combat.players[actor].turn_number, 30.0, true);
    buffers.state_numeric_values[SCALAR_SLOTS + 3] = 0;
    buffers.state_numeric_scales[SCALAR_SLOTS + 3] = 0.0;
    // :745-842 has playerPowers + enemies, no allies/pets entity collection.
    // Keep actor rows and enemy rows; omit extension players and their powers.
    let player_kind = tokens::kind_id("player").unwrap();
    let enemy_kind = tokens::kind_id("enemy").unwrap();
    let power_kind = tokens::kind_id("power").unwrap();
    let relic_kind = tokens::kind_id("relic").unwrap();
    let mut remap = [None; STATE_CAPACITY];
    let mut omit_allies = false;
    // The base wire appends legacy unknown-counter duplicates after the
    // ordinary relic object rows. Preserve the owner for both publications.
    let mut relic_owners=combat.run.relics.iter().chain(combat.run.relics.iter()
        .filter(|relic|super::encoder::relic_public_counter(relic.id,relic.counter).is_some()));
    let mut count = 0;
    for old in 0..buffers.state_rows {
        let kind = buffers.state_kind_ids[old];
        if old > 1 && kind == player_kind { omit_allies = true; }
        if kind == enemy_kind || (kind != player_kind && kind != power_kind) { omit_allies = false; }
        let keep = if kind == relic_kind {
            relic_owners.next().ok_or(ObsError::UnsupportedState("relic row owner missing"))?.owner==actor
        } else { !omit_allies };
        if !keep { continue; }
        remap[old] = Some(count);
        buffers.state_kind_ids[count] = kind;
        buffers.state_mask[count] = buffers.state_mask[old];
        buffers.state_token_ids.copy_within(old * TOKEN_SLOTS..(old + 1) * TOKEN_SLOTS, count * TOKEN_SLOTS);
        buffers.state_numeric_values.copy_within(old * SCALAR_SLOTS..(old + 1) * SCALAR_SLOTS, count * SCALAR_SLOTS);
        buffers.state_numeric_scales.copy_within(old * SCALAR_SLOTS..(old + 1) * SCALAR_SLOTS, count * SCALAR_SLOTS);
        buffers.state_numeric_tail.copy_within(old * BOOLEAN_FEATURES..(old + 1) * BOOLEAN_FEATURES, count * BOOLEAN_FEATURES);
        count += 1;
    }
    for (links, known) in [
        (&mut *buffers.source_links_bits, &mut *buffers.source_known),
        (&mut *buffers.target_links_bits, &mut *buffers.target_known),
    ] {
        for candidate in 0..buffers.candidate_rows {
            let start = candidate * ROUTE_BYTES_PER_CANDIDATE;
            let old: [u8; ROUTE_BYTES_PER_CANDIDATE] =
                links[start..start + ROUTE_BYTES_PER_CANDIDATE].try_into().unwrap();
            links[start..start + ROUTE_BYTES_PER_CANDIDATE].fill(0);
            let mut omitted = false;
            let mut kept = false;
            for row in 0..buffers.state_rows {
                if old[row / 8] & (1 << (row % 8)) == 0 { continue; }
                if let Some(new) = remap[row] {
                    links[start + new / 8] |= 1 << (new % 8);
                    kept = true;
                } else { omitted = true; }
            }
            if omitted && !kept { known[candidate] = 0; }
        }
    }
    buffers.state_kind_ids[count..STATE_CAPACITY].fill(0);
    buffers.state_mask[count..STATE_CAPACITY].fill(0);
    buffers.state_token_ids[count * TOKEN_SLOTS..STATE_CAPACITY * TOKEN_SLOTS].fill(0);
    buffers.state_numeric_values[count * SCALAR_SLOTS..STATE_CAPACITY * SCALAR_SLOTS].fill(0);
    buffers.state_numeric_scales[count * SCALAR_SLOTS..STATE_CAPACITY * SCALAR_SLOTS].fill(0.0);
    buffers.state_numeric_tail[count * BOOLEAN_FEATURES..STATE_CAPACITY * BOOLEAN_FEATURES].fill(0.0);
    buffers.state_rows = count;
    Ok(())
}

/// Run adapter: combat uses the per-seat view; supported legacy run menus use
/// their unchanged encoder. Foreign-seat menus fail closed (not simulated).
pub fn write_run_perspective(
    run: &RunManager, actor: CId, context: &CombatObservationContext<'_>,
    buffers: &mut ObsBuffers<'_>, slots: &mut ActionSlots,
) -> Result<(), ObsError> {
    if run.phase() == RunPhase::Combat {
        return write_combat_perspective(run.combat().ok_or(
            ObsError::UnsupportedState("combat phase without combat"))?, actor, context, buffers, slots);
    }
    if actor != 0 { return Err(ObsError::UnsupportedState("run menus belong to seat zero")); }
    let mut view = run.clone();
    let actions = view.available_actions();
    write_neow_obs(&view, &actions, context, buffers, slots)
}
