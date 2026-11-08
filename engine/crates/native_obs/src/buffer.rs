use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionStatus { Actionable, Waiting, Terminal, GameplayFault }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionScope { Seat(u8), TeamCombat }

pub const ACTION_CONTRACT: &str = "sts2-native-actions-v2";
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionIdentity {
    pub contract: String,
    pub episode: String,
    pub generation: String,
    pub actor: u8,
    pub slot: i32,
    pub binding: String,
}
impl ActionIdentity {
    pub fn new(decision: DecisionId, actor: u8, slot: i32, binding: impl core::fmt::Debug) -> Self {
        Self { contract: ACTION_CONTRACT.into(), episode: decision.episode.to_string(), generation: decision.generation.to_string(),
            actor, slot, binding: format!("{binding:?}") }
    }
}

/// Borrowed learned inputs. Simulator bindings and absolute instance IDs are excluded.
pub struct ObservationView<'a> {
    pub decision: DecisionId,
    pub scope: DecisionScope,
    pub identity: ManifestIdentity,
    pub status: DecisionStatus,
    pub state_len: usize,
    pub candidate_len: usize,
    pub rows: RowsView<'a>,
    pub candidates: CandidatesView<'a>,
    pub edges: &'a [Edge],
    pub legacy: Option<LegacyObservationView<'a>>,
    pub streamed_piles: StreamedPilesView<'a>,
}
/// Exact pre-V2 learned inputs. Content identities are compared by namespace
/// and native ordinal; this sidecar never changes V2 checkpoint admission.
#[derive(Clone, Copy)]
pub struct LegacyObservationView<'a> {
    pub decision: DecisionId,
    pub scope: DecisionScope,
    pub status: DecisionStatus,
    pub state_len: usize,
    pub candidate_len: usize,
    pub rows: RowsView<'a>,
    pub candidates: CandidatesView<'a>,
    pub edges: &'a [Edge],
}
impl<'a> From<ObservationView<'a>> for LegacyObservationView<'a> {
    fn from(view: ObservationView<'a>) -> Self {
        Self { decision: view.decision, scope: view.scope, status: view.status,
            state_len: view.state_len, candidate_len: view.candidate_len,
            rows: view.rows, candidates: view.candidates, edges: view.edges }
    }
}

/// Fixed columns allocate once per lane; overflow streams reuse growing capacity.
/// Data is published only after finish; any failure poisons the snapshot until clear.
pub struct NativeObsV2 {
    decision: DecisionId,
    state_len: u16,
    candidate_len: u16,
    edge_len: u16,
    selection_len: u16,
    rows: Rows,
    candidates: Candidates,
    edges: Box<[Edge]>,
    bindings: Box<[Binding]>,
    selection: Box<[SelectionItem]>,
    status: Option<DecisionStatus>,
    error: Option<ContractError>,
    // A count/validation visitor borrows the publication without overwriting
    // columns. It traverses exactly the same rows, edges and command stream.
    check: Option<BindingCheck>,
    pub(crate) legacy: Option<Box<NativeObsV2>>,
    legacy_valid: bool,
    pub(crate) streamed_piles: crate::pile_stream::PileStream,
}
struct BindingCheck {
    index: usize,
    expected: Binding,
    next: usize,
    selections: usize,
    matched: bool,
    status: Option<DecisionStatus>,
    rows: usize,
    edges: usize,
    first: RowKind,
    actionable: bool,
}
/// Source compatibility only: this alias publishes the V2 manifest and cannot
/// admit V1 weights or bypass checkpoint refitting.
pub type NativeObsV1 = NativeObsV2;
impl Default for NativeObsV2 { fn default() -> Self { Self::new() } }
impl NativeObsV2 {
    pub fn new() -> Self {
        Self {
            decision: DecisionId::default(), state_len: 0, candidate_len: 0, edge_len: 0, selection_len: 0,
            rows: Rows::new(), candidates: Candidates::new(), edges: vec![Edge::EMPTY; EDGE_CAPACITY].into_boxed_slice(),
            bindings: vec![Binding::default(); CANDIDATE_CAPACITY].into_boxed_slice(),
            selection: vec![SelectionItem::default(); SELECTION_CAPACITY].into_boxed_slice(), status: None, error: None, check: None,
            legacy: None, legacy_valid: false,
            streamed_piles: Default::default(),
        }
    }
    pub fn clear(&mut self, decision: DecisionId) {
        if self.check.is_some() { return; }
        self.decision = decision;
        self.state_len = 0; self.candidate_len = 0; self.edge_len = 0; self.selection_len = 0;
        self.candidates.candidate_present.fill(0); self.candidates.legal.fill(0);
        self.status = None;
        self.legacy_valid = false;
        self.streamed_piles.clear();
        self.error = (usize::from(decision.actor) >= MAX_SEATS).then_some(ContractError::InvalidSeat);
    }
    pub(crate) fn begin_binding_check(&mut self, index: usize, expected: Binding) {
        self.check = Some(BindingCheck { index, expected, next: 0, selections: 0, matched: false, status: None,
            rows: 0, edges: 0, first: RowKind::Absent, actionable: false });
    }
    pub(crate) fn checking_binding(&self) -> bool { self.check.is_some() }
    pub(crate) fn end_binding_check(&mut self) -> Result<(), ContractError> {
        let check = self.check.take().ok_or(ContractError::InvalidBinding)?;
        if check.matched && check.status == Some(DecisionStatus::Actionable) { Ok(()) }
        else { Err(ContractError::StaleBinding) }
    }
    pub fn lengths(&self) -> (usize, usize, usize, usize) {
        (self.state_len as usize, self.candidate_len as usize, self.edge_len as usize, self.selection_len as usize)
    }
    fn fail<T>(&mut self, error: ContractError) -> Result<T, ContractError> {
        if self.check.is_some() { return Err(error); }
        self.status = None;
        let first = *self.error.get_or_insert(error);
        Err(first)
    }
    fn begin_write(&mut self) -> Result<(), ContractError> {
        if self.check.is_some() { return Ok(()); }
        if let Some(error) = self.error { return Err(error); }
        self.status = None;
        self.legacy_valid = false;
        Ok(())
    }
    fn handle_valid(&self, handle: RowHandle) -> bool {
        usize::from(handle.0) <= self.check.as_ref().map_or(usize::from(self.state_len), |check| check.rows)
    }
    pub fn push_row(&mut self, mut row: Row) -> Result<RowHandle, ContractError> {
        self.begin_write()?;
        if self.check.as_ref().map_or(self.state_len as usize, |check| check.rows) == STATE_CAPACITY {
            return self.fail(ContractError::Capacity { buffer: BufferKind::Rows, needed: STATE_CAPACITY + 1, capacity: STATE_CAPACITY });
        }
        if row.kind == RowKind::Absent { return self.fail(ContractError::Incomplete); }
        if row.multiplicity == 0 { return self.fail(ContractError::InvalidNumericField); }
        if !row.model.is_valid() || row.detail.iter().any(|id| !id.is_valid()) {
            return self.fail(ContractError::InvalidContentId);
        }
        if !self.handle_valid(row.parent) || !self.handle_valid(row.owner) {
            return self.fail(ContractError::InvalidHandle);
        }
        if let Err(error) = row.numbers.validate(ROW_FIELDS[row.kind as usize]).and(row.flags.validate()) {
            return self.fail(error);
        }
        if let Some(check) = &mut self.check {
            if check.rows == 0 { check.first = row.kind; }
            check.rows += 1;
            return Ok(RowHandle(check.rows as u16));
        }
        if matches!(row.kind, RowKind::CardPreview | RowKind::UpgradePreview | RowKind::PileSummary
            | RowKind::RouteSummary | RowKind::Boss | RowKind::EffectParameter | RowKind::Modal | RowKind::ModalOption)
            || row.parent.index().is_some_and(|i| self.rows.legacy_role[i] == LegacyRowRole::V2Only) {
            row.legacy_role = LegacyRowRole::V2Only;
        }
        if row.multiplicity == 1 && let Some(parent) = row.parent.index() {
            row.multiplicity = self.rows.multiplicity[parent];
        }
        self.rows.put(self.state_len as usize, row);
        self.state_len += 1;
        Ok(RowHandle(self.state_len))
    }
    pub fn push_candidate(&mut self, candidate: Candidate, binding: Binding) -> Result<u16, ContractError> {
        self.push_candidate_inner(candidate, Some(binding))
    }
    /// Learned inputs only. A live-game option has no simulator dispatch authority.
    pub fn push_advisor_candidate(&mut self, candidate: Candidate) -> Result<u16, ContractError> {
        if self.check.is_some() { return Err(ContractError::InvalidBinding); }
        self.push_candidate_inner(candidate, None)
    }
    fn push_candidate_inner(&mut self, candidate: Candidate, binding: Option<Binding>) -> Result<u16, ContractError> {
        self.begin_write()?;
        if self.check.as_ref().map_or(self.candidate_len as usize, |check| check.next) == CANDIDATE_CAPACITY {
            return self.fail(ContractError::Capacity { buffer: BufferKind::Candidates, needed: CANDIDATE_CAPACITY + 1, capacity: CANDIDATE_CAPACITY });
        }
        if candidate.actor != self.decision.actor { return self.fail(ContractError::WrongOwner); }
        if candidate.present && (candidate.action == ActionKind::Absent ||
            binding.is_some_and(|b|b.command.action() != candidate.action)) {
            return self.fail(ContractError::InvalidBinding);
        }
        if candidate.present && let Some(binding) = binding && let Err(error) = binding.validate_payload() {
            return self.fail(error);
        }
        if candidate.present && let Some(binding) = binding && binding.decision != self.decision {
            return self.fail(if binding.decision.actor != self.decision.actor {
                ContractError::WrongOwner
            } else { ContractError::StaleBinding });
        }
        if !candidate.model.is_valid() || candidate.detail.iter().any(|id| !id.is_valid()) {
            return self.fail(ContractError::InvalidContentId);
        }
        if !self.handle_valid(candidate.source) || !self.handle_valid(candidate.target) {
            return self.fail(ContractError::InvalidHandle);
        }
        if let Some(span) = binding.and_then(|b|b.command.selection()) {
            if usize::from(span.start) + usize::from(span.len)
                > self.check.as_ref().map_or(usize::from(self.selection_len), |check| check.selections) {
                return self.fail(ContractError::InvalidBinding);
            }
        }
        if let Err(error) = candidate.numbers.validate(CANDIDATE_FIELDS).and(candidate.flags.validate()) {
            return self.fail(error);
        }
        if let Some(check) = &mut self.check {
            let index = check.next;
            if index == check.index {
                check.matched = candidate.present && candidate.legal && binding == Some(check.expected)
                    && candidate.model == self.candidates.model[index]
                    && candidate.detail == self.candidates.detail[index];
            }
            check.actionable |= candidate.present && candidate.legal;
            check.next += 1;
            return Ok(index as u16);
        }
        let index = self.candidate_len;
        self.candidates.put(index as usize, candidate);
        self.bindings[index as usize] = binding.unwrap_or_default();
        self.candidate_len += 1;
        Ok(index)
    }
    pub fn push_edge(&mut self, edge: Edge) -> Result<(), ContractError> {
        self.begin_write()?;
        if self.check.as_ref().map_or(self.edge_len as usize, |check| check.edges) == EDGE_CAPACITY {
            return self.fail(ContractError::Capacity { buffer: BufferKind::Edges, needed: EDGE_CAPACITY + 1, capacity: EDGE_CAPACITY });
        }
        if edge.from == RowHandle::ABSENT || edge.to == RowHandle::ABSENT
            || !self.handle_valid(edge.from) || !self.handle_valid(edge.to) {
            return self.fail(ContractError::InvalidHandle);
        }
        if let Some(check) = &mut self.check { check.edges += 1; return Ok(()); }
        self.edges[self.edge_len as usize] = edge;
        self.edge_len += 1;
        Ok(())
    }
    pub fn push_selection(&mut self, selected: &[SelectionItem]) -> Result<SelectionSpan, ContractError> {
        if let Some(check) = &mut self.check {
            let start = check.selections;
            let end = start.checked_add(selected.len()).ok_or(ContractError::InvalidBinding)?;
            if end > SELECTION_CAPACITY {
                return Err(ContractError::Capacity { buffer: BufferKind::Selection, needed: end, capacity: SELECTION_CAPACITY });
            }
            if end > usize::from(self.selection_len) || self.selection[start..end] != *selected {
                return Err(ContractError::StaleBinding);
            }
            check.selections = end;
            return Ok(SelectionSpan { start: start as u16, len: selected.len() as u16 });
        }
        self.begin_write()?;
        let start = self.selection_len as usize;
        let needed = start.checked_add(selected.len()).ok_or(ContractError::InvalidBinding)?;
        if needed > SELECTION_CAPACITY {
            return self.fail(ContractError::Capacity { buffer: BufferKind::Selection, needed, capacity: SELECTION_CAPACITY });
        }
        self.selection[start..needed].copy_from_slice(selected);
        self.selection_len = needed as u16;
        Ok(SelectionSpan { start: start as u16, len: selected.len() as u16 })
    }
    pub fn finish(&mut self, status: DecisionStatus) -> Result<(), ContractError> {
        if let Some(check) = &mut self.check {
            if status == DecisionStatus::Actionable {
                if check.first != RowKind::Global { return Err(ContractError::MissingGlobal); }
                if !check.actionable { return Err(ContractError::EmptyActionable); }
            } else if check.next != 0 { return Err(ContractError::NonActionableCandidates); }
            check.status = Some(status); return Ok(());
        }
        self.begin_write()?;
        if status == DecisionStatus::Actionable {
            if self.state_len == 0 || self.rows.kind[0] != RowKind::Global {
                return self.fail(ContractError::MissingGlobal);
            }
            if !(0..self.candidate_len as usize).any(|i| self.candidates.sampleable(i, self.candidate_len as usize)) {
                return self.fail(ContractError::EmptyActionable);
            }
        } else if self.candidate_len != 0 {
            return self.fail(ContractError::NonActionableCandidates);
        }
        self.status = Some(status);
        Ok(())
    }
    pub fn view(&self) -> Result<ObservationView<'_>, ContractError> {
        if let Some(error) = self.error { return Err(error); }
        let status = self.status.ok_or(ContractError::Incomplete)?;
        let legacy = if self.legacy_valid {
            Some(LegacyObservationView::from(self.legacy.as_ref().ok_or(ContractError::Incomplete)?.view()?))
        } else { None };
        let mut candidates = self.candidates.view(self.candidate_len as usize);
        if let Some(old) = legacy {
            candidates.legacy_source = Some(old.candidates.source);
            candidates.legacy_target = Some(old.candidates.target);
        }
        Ok(ObservationView {
            decision: self.decision, scope: DecisionScope::Seat(self.decision.actor), identity: MANIFEST_IDENTITY, status,
            state_len: self.state_len as usize, candidate_len: self.candidate_len as usize,
            rows: self.rows.view(self.state_len as usize),
            candidates,
            edges: &self.edges[..self.edge_len as usize],
            legacy,
            streamed_piles: self.streamed_piles.view(),
        })
    }
    pub(crate) fn attach_legacy(&mut self, legacy: Box<NativeObsV2>) -> Result<(), ContractError> {
        if self.decision != legacy.decision || self.status != legacy.status
            || self.candidate_len != legacy.candidate_len || self.selection_len != legacy.selection_len
            || self.bindings[..self.candidate_len as usize] != legacy.bindings[..legacy.candidate_len as usize]
            || self.selection[..self.selection_len as usize] != legacy.selection[..legacy.selection_len as usize] {
            return self.fail(ContractError::InvalidBinding);
        }
        self.legacy = Some(legacy); self.legacy_valid = true;
        Ok(())
    }
    pub(crate) fn mark_v2_only(&mut self, handle: RowHandle) {
        if self.check.is_none() {
            self.rows.legacy_role[handle.index().expect("emitted row")] = LegacyRowRole::V2Only;
        }
    }
    pub fn binding(&self, index: usize, current: DecisionId, net_id: u64) -> Result<Binding, ContractError> {
        let view = self.view()?;
        if view.status != DecisionStatus::Actionable || !self.candidates.sampleable(index, view.candidate_len) {
            return Err(ContractError::InvalidBinding);
        }
        let binding = self.bindings[index];
        binding.validate_owner(current, net_id)?;
        Ok(binding)
    }
    pub fn selection(&self, span: SelectionSpan) -> Result<&[SelectionItem], ContractError> {
        self.view()?;
        let end = usize::from(span.start) + usize::from(span.len);
        if end > usize::from(self.selection_len) { return Err(ContractError::InvalidBinding); }
        Ok(&self.selection[usize::from(span.start)..end])
    }
}
