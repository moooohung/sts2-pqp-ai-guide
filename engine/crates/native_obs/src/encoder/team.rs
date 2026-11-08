use super::*;
use crate::buffer::DecisionScope;

pub const TEAM_CANDIDATE_CAPACITY: usize = MAX_SEATS * CANDIDATE_CAPACITY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamAction {
    Seat { original_slot: usize, binding: Binding },
    UnendTurn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamBinding {
    pub actor: u8,
    pub net_id: u64,
    pub action: TeamAction,
}
#[derive(Default)]
struct Columns {
    action: Vec<ActionKind>, model: Vec<ContentId>, detail: Vec<[ContentId; ID_LANES]>,
    source: Vec<RowHandle>, target: Vec<RowHandle>, actor: Vec<u8>,
    flags: Vec<u64>, flags_present: Vec<u64>,
    integers: Vec<[i32; NUMERIC_LANES]>, floats: Vec<[f32; NUMERIC_LANES]>,
    integer_present: Vec<u16>, float_present: Vec<u16>, present: Vec<u64>, legal: Vec<u64>,
}
impl Columns {
    fn build(candidates: &[Candidate]) -> Self {
        let mut out = Self::default();
        out.present.resize(candidates.len().div_ceil(64), 0);
        out.legal.resize(candidates.len().div_ceil(64), 0);
        for (i, c) in candidates.iter().enumerate() {
            out.action.push(c.action); out.model.push(c.model); out.detail.push(c.detail);
            out.source.push(c.source); out.target.push(c.target); out.actor.push(c.actor);
            out.flags.push(c.flags.value); out.flags_present.push(c.flags.present);
            out.integers.push(c.numbers.integers); out.floats.push(c.numbers.floats);
            out.integer_present.push(c.numbers.integer_present); out.float_present.push(c.numbers.float_present);
            if c.present { out.present[i / 64] |= 1 << (i % 64); }
            if c.legal { out.legal[i / 64] |= 1 << (i % 64); }
        }
        out
    }
    fn view(&self) -> CandidatesView<'_> {
        CandidatesView { action: &self.action, model: &self.model, detail: &self.detail,
            source: &self.source, target: &self.target, actor: &self.actor,
            flags: &self.flags, flags_present: &self.flags_present, integers: &self.integers,
            floats: &self.floats, integer_present: &self.integer_present, float_present: &self.float_present,
            candidate_present: &self.present, legal: &self.legal, legacy_source: None, legacy_target: None }
    }
}

/// Seat buffers use the V2 manifest; only this team publication shares hands.
pub struct TeamObsV2 {
    decision: DecisionId, rows: Rows, state_len: usize, edges: Vec<Edge>,
    candidates: Vec<Candidate>, columns: Columns, bindings: Vec<Option<TeamBinding>>,
    status: Option<DecisionStatus>,
    legacy: Option<Box<TeamObsV2>>,
    streamed_piles: crate::pile_stream::PileStream,
}
impl Default for TeamObsV2 { fn default() -> Self { Self::new() } }
impl TeamObsV2 {
    pub fn new() -> Self {
        Self { decision: DecisionId::default(), rows: Rows::new(), state_len: 0,
            edges: Vec::new(), candidates: Vec::new(), columns: Columns::default(),
            bindings: Vec::new(), status: None, legacy: None, streamed_piles: Default::default() }
    }
    pub fn view(&self) -> core::result::Result<ObservationView<'_>, ContractError> {
        let legacy = self.legacy.as_ref().map(|old| old.view().map(LegacyObservationView::from)).transpose()?;
        let mut candidates = self.columns.view();
        if let Some(old) = legacy {
            candidates.legacy_source = Some(old.candidates.source);
            candidates.legacy_target = Some(old.candidates.target);
        }
        Ok(ObservationView { decision: self.decision, scope: DecisionScope::TeamCombat,
            identity: TEAM_MANIFEST_IDENTITY, status: self.status.ok_or(ContractError::Incomplete)?,
            state_len: self.state_len, candidate_len: self.candidates.len(),
            rows: self.rows.view(self.state_len), candidates, edges: &self.edges, legacy,
            streamed_piles: self.streamed_piles.view() })
    }
    pub fn binding(&self, slot: usize, current: DecisionId) -> core::result::Result<TeamBinding, ContractError> {
        let view = self.view()?;
        if current != self.decision { return Err(ContractError::StaleBinding); }
        if view.status != DecisionStatus::Actionable || !view.candidates.sampleable(slot, view.candidate_len) {
            return Err(ContractError::InvalidBinding);
        }
        self.bindings.get(slot).copied().flatten().ok_or(ContractError::InvalidBinding)
    }
    fn row(&mut self, row: Row) -> Result<()> {
        if self.state_len == STATE_CAPACITY {
            return Err(capacity(BufferKind::Rows, self.state_len + 1, STATE_CAPACITY));
        }
        self.rows.put(self.state_len, row); self.state_len += 1; Ok(())
    }
    fn candidate(&mut self, candidate: Candidate, binding: Option<TeamBinding>) -> Result<()> {
        if self.candidates.len() == TEAM_CANDIDATE_CAPACITY {
            return Err(capacity(BufferKind::Candidates, self.candidates.len() + 1, TEAM_CANDIDATE_CAPACITY));
        }
        self.candidates.push(candidate); self.bindings.push(binding); Ok(())
    }
}
fn row_at(view: &ObservationView<'_>, i: usize) -> Row {
    let r = view.rows;
    Row { kind: r.kind[i], model: r.model[i], detail: r.detail[i], owner: r.owner[i], parent: r.parent[i],
        flags: KnownFlags { value: r.flags[i], present: r.flags_present[i] },
        numbers: Numbers { integers: r.integers[i], floats: r.floats[i],
            integer_present: r.integer_present[i], float_present: r.float_present[i] },
        legacy_role: r.legacy_role[i], multiplicity: r.multiplicity[i] }
}

impl NativeEncoder {
    pub fn advance_team(run: &mut MpRun, decision: &mut DecisionId) -> core::result::Result<bool, DispatchError> {
        let mut advanced = Self::advance_multiplayer(run, decision)?;
        if run.synchronize_combat_queue().map_err(DispatchError::Gameplay)? {
            decision.generation=decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            advanced=true;
        }
        for owner in 0..run.roster.players().len() {
            if run.phase != sts2_sim::run::mp::MpPhase::Combat
                || !run.combat().is_some_and(|c| c.needs_enemy_turn_acknowledgement(owner as u8)) { continue; }
            let net = run.roster.players()[owner].net_id;
            run.ready_to_begin_enemy_turn_for(net, owner).map_err(DispatchError::Gameplay)?;
            decision.generation = decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            advanced = true;
        }
        advanced |= Self::advance_multiplayer(run, decision)?;
        if run.synchronize_combat_queue().map_err(DispatchError::Gameplay)? {
            decision.generation=decision.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
            advanced=true;
        }
        Ok(advanced)
    }

    pub fn encode_team(&mut self, run: &MpRun, decision: DecisionId, scratch: &mut NativeObsV1,
        team: &mut TeamObsV2) -> Result<()> {
        let old = team.legacy.take();
        self.team_decision(run, decision, scratch, team)?;
        if let Some(encoder) = &mut self.legacy {
            encoder.ui = self.ui.clone();
            let mut old = old.unwrap_or_default();
            let mut old_scratch = scratch.legacy.take().unwrap_or_default();
            let result = encoder.team_decision(run, decision, &mut old_scratch, &mut old);
            if matches!(result, Err(EncodeError::Contract(ContractError::Capacity {
                buffer: BufferKind::Rows | BufferKind::Edges, .. }))) {
                scratch.legacy = Some(old_scratch);
                return Ok(());
            }
            result?;
            scratch.attach_legacy(old_scratch)?;
            if team.candidates.len() != old.candidates.len() || team.bindings != old.bindings
                || team.status != old.status {
                team.status = None;
                return Err(ContractError::InvalidBinding.into());
            }
            team.legacy = Some(old);
        }
        Ok(())
    }
    fn team_decision(&mut self, run: &MpRun, decision: DecisionId, scratch: &mut NativeObsV1,
        team: &mut TeamObsV2) -> Result<()> {
        if run.phase != sts2_sim::run::mp::MpPhase::Combat || run.roster.players().len() < 2 || decision.actor != 0 {
            return Err(EncodeError::InvalidState("team scope requires multiplayer combat and canonical anchor"));
        }
        team.status = None; team.state_len = 0; team.candidates.clear(); team.bindings.clear();
        team.edges.clear(); team.decision = decision;
        team.streamed_piles.clear();
        self.team = true;
        let result = (|| {
            let mut base = 0;
            for owner in 0..run.roster.players().len() {
                let net = run.roster.players()[owner].net_id;
                let current = DecisionId { actor: owner as u8, ..decision };
                self.multiplayer_decision(run, current, net, scratch)?;
                let view = scratch.view()?;
                let end = self.combat_state_len;
                if owner == 0 {
                    base = end;
                    for i in 0..base { team.row(row_at(&view, i))?; }
                    team.streamed_piles.append(view.streamed_piles);
                } else if base != end { return Err(EncodeError::InvalidState("team state boundary changed by perspective")); }
                let extra_start = team.state_len;
                let route = |handle: RowHandle| {
                    match handle.index() {
                        Some(i) if i >= base => RowHandle((extra_start + i - base + 1) as u16),
                        _ => handle,
                    }
                };
                for i in base..view.state_len {
                    let mut row = row_at(&view, i);
                    row.owner = route(row.owner); row.parent = route(row.parent); team.row(row)?;
                }
                for edge in view.edges {
                    if owner == 0 || edge.from.index().is_some_and(|i| i >= base) {
                        if team.edges.len() == EDGE_CAPACITY {
                            return Err(capacity(BufferKind::Edges, team.edges.len() + 1, EDGE_CAPACITY));
                        }
                        team.edges.push(Edge { from: route(edge.from), to: route(edge.to), relation: edge.relation });
                    }
                }
                for i in 0..view.candidate_len {
                    let c = view.candidates;
                    if c.action[i] == ActionKind::ReadyForEnemyTurn { continue; }
                    let candidate = Candidate { action: c.action[i], model: c.model[i], detail: c.detail[i],
                        source: route(c.source[i]), target: route(c.target[i]), actor: owner as u8,
                        flags: KnownFlags { value: c.flags[i], present: c.flags_present[i] },
                        numbers: Numbers { integers: c.integers[i], floats: c.floats[i],
                            integer_present: c.integer_present[i], float_present: c.float_present[i] },
                        present: c.candidate_present[i / 64] & (1 << (i % 64)) != 0,
                        legal: c.legal[i / 64] & (1 << (i % 64)) != 0 };
                    let binding = if c.sampleable(i, view.candidate_len) {
                        Some(TeamBinding { actor: owner as u8, net_id: net, action: TeamAction::Seat {
                            original_slot: i, binding: scratch.binding(i, current, net)? } })
                    } else { None };
                    team.candidate(candidate, binding)?;
                }
                if run.combat().is_some_and(|c| c.can_unend_player_turn(owner as u8)) {
                    team.candidate(Candidate { action: ActionKind::UnendTurn, actor: owner as u8,
                        source: self.seat_rows[owner], present: true, legal: true, ..Candidate::default() },
                        Some(TeamBinding { actor: owner as u8, net_id: net, action: TeamAction::UnendTurn }))?;
                }
            }
            team.columns = Columns::build(&team.candidates);
            team.status = Some(if run.combat().is_some_and(|c| c.fault.is_some()) { DecisionStatus::GameplayFault }
                else if run.combat().is_some_and(|c| c.is_over) { DecisionStatus::Terminal }
                else if team.candidates.iter().any(|c| c.present && c.legal) { DecisionStatus::Actionable }
                else { DecisionStatus::Waiting });
            Ok(())
        })();
        self.team = false;
        result
    }

    pub fn dispatch_team(&mut self, run: &mut MpRun, decision: &mut DecisionId,
        scratch: &mut NativeObsV1, team: &mut TeamObsV2, slot: usize) -> core::result::Result<(), DispatchError> {
        let expected = team.binding(slot, *decision)?;
        run.roster.actor(expected.net_id, expected.actor as usize).map_err(DispatchError::Gameplay)?;
        self.encode_team(run, *decision, scratch, team)?;
        if team.binding(slot, *decision)? != expected { return Err(ContractError::StaleBinding.into()); }
        let mut current = DecisionId { actor: expected.actor, ..*decision };
        let result = match expected.action {
            TeamAction::UnendTurn => {
                run.unend_turn_for(expected.net_id, expected.actor as usize).map_err(DispatchError::Gameplay)?;
                current.generation = current.generation.checked_add(1).ok_or(DispatchError::GenerationOverflow)?;
                Ok(())
            }
            TeamAction::Seat { original_slot, binding } => {
                self.team = true;
                let result = (|| {
                    self.multiplayer_decision(run, current, expected.net_id, scratch)?;
                    if scratch.binding(original_slot, current, expected.net_id)? != binding {
                        return Err(ContractError::StaleBinding.into());
                    }
                    self.dispatch_multiplayer(run, &mut current, expected.net_id, scratch, original_slot)
                })();
                self.team = false; result
            }
        };
        if result.is_ok() { *decision = DecisionId { actor: 0, ..current }; team.status = None; }
        result
    }
}
