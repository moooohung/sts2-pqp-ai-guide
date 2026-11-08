use crate::*;

/// One-based observation-local row reference; never a learned content ID.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RowHandle(pub u16);
impl RowHandle {
    pub const ABSENT: Self = Self(0);
    pub const fn index(self) -> Option<usize> {
        if self.0 == 0 { None } else { Some(self.0 as usize - 1) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KnownFlags { pub value: u64, pub present: u64 }
impl KnownFlags {
    pub fn set(&mut self, flag: Flag, value: bool) {
        self.present |= flag.bit();
        if value { self.value |= flag.bit(); } else { self.value &= !flag.bit(); }
    }
    pub fn get(self, flag: Flag) -> Option<bool> {
        (self.present & flag.bit() != 0).then_some(self.value & flag.bit() != 0)
    }
    pub(crate) fn validate(self) -> Result<(), ContractError> {
        if self.present & !VALID_FLAGS != 0 || self.value & !self.present != 0 {
            return Err(ContractError::InvalidFlags);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Numbers {
    pub integers: [i32; NUMERIC_LANES],
    pub floats: [f32; NUMERIC_LANES],
    pub integer_present: u16,
    pub float_present: u16,
}
impl Numbers {
    pub const EMPTY: Self = Self {
        integers: [0; NUMERIC_LANES], floats: [0.; NUMERIC_LANES],
        integer_present: 0, float_present: 0,
    };
    pub fn set_integer(&mut self, lane: usize, value: i32) -> Result<(), ContractError> {
        if lane >= NUMERIC_LANES { return Err(ContractError::InvalidNumericField); }
        let bit = 1u16 << lane;
        self.integers[lane] = value;
        self.floats[lane] = 0.;
        self.integer_present |= bit;
        self.float_present &= !bit;
        Ok(())
    }
    pub fn set_float(&mut self, lane: usize, value: f32) -> Result<(), ContractError> {
        if lane >= NUMERIC_LANES { return Err(ContractError::InvalidNumericField); }
        if !value.is_finite() { return Err(ContractError::NonFinite); }
        let bit = 1u16 << lane;
        self.floats[lane] = value;
        self.integers[lane] = 0;
        self.float_present |= bit;
        self.integer_present &= !bit;
        Ok(())
    }
    pub(crate) fn validate(self, schema: &[NumericField]) -> Result<(), ContractError> {
        if self.integer_present & self.float_present != 0 {
            return Err(ContractError::InvalidNumericField);
        }
        for lane in 0..NUMERIC_LANES {
            let integer = self.integer_present & (1 << lane) != 0;
            let float = self.float_present & (1 << lane) != 0;
            if !self.floats[lane].is_finite() { return Err(ContractError::NonFinite); }
            if (!integer && self.integers[lane] != 0) || (!float && self.floats[lane] != 0.) {
                return Err(ContractError::InvalidNumericField);
            }
            if integer || float {
                let field = schema.get(lane).ok_or(ContractError::InvalidNumericField)?;
                if (integer && field.kind == NumericKind::Float) || (float && field.kind == NumericKind::Integer) {
                    return Err(ContractError::InvalidNumericField);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    pub model: ContentId,
    pub detail: [ContentId; ID_LANES],
    pub owner: RowHandle,
    pub parent: RowHandle,
    pub flags: KnownFlags,
    pub numbers: Numbers,
    pub legacy_role: LegacyRowRole,
    pub multiplicity: u32,
}
impl Default for Row {
    fn default() -> Self {
        Self { kind: RowKind::Absent, model: ContentId::ABSENT, detail: [ContentId::ABSENT; ID_LANES],
            owner: RowHandle::ABSENT, parent: RowHandle::ABSENT, flags: KnownFlags::default(),
            numbers: Numbers::EMPTY, legacy_role: LegacyRowRole::Shared, multiplicity: 1 }
    }
}
/// Migration metadata, not an input to the learned row encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegacyRowRole { #[default] Shared, V2Only }
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Candidate {
    pub action: ActionKind,
    pub model: ContentId,
    pub detail: [ContentId; ID_LANES],
    pub source: RowHandle,
    pub target: RowHandle,
    pub actor: u8,
    pub flags: KnownFlags,
    pub numbers: Numbers,
    pub present: bool,
    pub legal: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge { pub from: RowHandle, pub to: RowHandle, pub relation: Relation }
impl Edge {
    pub const EMPTY: Self = Self { from: RowHandle::ABSENT, to: RowHandle::ABSENT, relation: Relation::Parent };
}

/// Only populated columns are borrowed; unused capacity is never policy-visible.
#[derive(Clone, Copy, Debug)]
pub struct RowsView<'a> {
    pub kind: &'a [RowKind],
    pub model: &'a [ContentId],
    pub detail: &'a [[ContentId; ID_LANES]],
    pub owner: &'a [RowHandle],
    pub parent: &'a [RowHandle],
    pub flags: &'a [u64],
    pub flags_present: &'a [u64],
    pub integers: &'a [[i32; NUMERIC_LANES]],
    pub floats: &'a [[f32; NUMERIC_LANES]],
    pub integer_present: &'a [u16],
    pub float_present: &'a [u16],
    pub legacy_role: &'a [LegacyRowRole],
    pub multiplicity: &'a [u32],
}

/// Columns are caller-owned. Row-local numeric lanes remain contiguous for row MLPs.
pub struct Rows {
    pub kind: Box<[RowKind]>,
    pub model: Box<[ContentId]>,
    pub detail: Box<[[ContentId; ID_LANES]]>,
    pub owner: Box<[RowHandle]>,
    pub parent: Box<[RowHandle]>,
    pub flags: Box<[u64]>,
    pub flags_present: Box<[u64]>,
    pub integers: Box<[[i32; NUMERIC_LANES]]>,
    pub floats: Box<[[f32; NUMERIC_LANES]]>,
    pub integer_present: Box<[u16]>,
    pub float_present: Box<[u16]>,
    pub legacy_role: Box<[LegacyRowRole]>,
    pub multiplicity: Box<[u32]>,
}
impl Rows {
    pub(crate) fn view(&self, len: usize) -> RowsView<'_> {
        RowsView {
            kind: &self.kind[..len], model: &self.model[..len], detail: &self.detail[..len],
            owner: &self.owner[..len], parent: &self.parent[..len],
            flags: &self.flags[..len], flags_present: &self.flags_present[..len],
            integers: &self.integers[..len], floats: &self.floats[..len],
            integer_present: &self.integer_present[..len], float_present: &self.float_present[..len],
            legacy_role: &self.legacy_role[..len],
            multiplicity: &self.multiplicity[..len],
        }
    }
    pub(crate) fn new() -> Self {
        Self {
            kind: vec![RowKind::Absent; STATE_CAPACITY].into_boxed_slice(), model: vec![ContentId::ABSENT; STATE_CAPACITY].into_boxed_slice(),
            detail: vec![[ContentId::ABSENT; ID_LANES]; STATE_CAPACITY].into_boxed_slice(),
            owner: vec![RowHandle::ABSENT; STATE_CAPACITY].into_boxed_slice(), parent: vec![RowHandle::ABSENT; STATE_CAPACITY].into_boxed_slice(),
            flags: vec![0; STATE_CAPACITY].into_boxed_slice(), flags_present: vec![0; STATE_CAPACITY].into_boxed_slice(),
            integers: vec![[0; NUMERIC_LANES]; STATE_CAPACITY].into_boxed_slice(), floats: vec![[0.; NUMERIC_LANES]; STATE_CAPACITY].into_boxed_slice(),
            integer_present: vec![0; STATE_CAPACITY].into_boxed_slice(), float_present: vec![0; STATE_CAPACITY].into_boxed_slice(),
            legacy_role: vec![LegacyRowRole::Shared; STATE_CAPACITY].into_boxed_slice(),
            multiplicity: vec![1; STATE_CAPACITY].into_boxed_slice(),
        }
    }
    pub(crate) fn put(&mut self, index: usize, row: Row) {
        self.kind[index] = row.kind; self.model[index] = row.model; self.detail[index] = row.detail;
        self.owner[index] = row.owner; self.parent[index] = row.parent;
        self.flags[index] = row.flags.value; self.flags_present[index] = row.flags.present;
        self.integers[index] = row.numbers.integers; self.floats[index] = row.numbers.floats;
        self.integer_present[index] = row.numbers.integer_present; self.float_present[index] = row.numbers.float_present;
        self.legacy_role[index] = row.legacy_role;
        self.multiplicity[index] = row.multiplicity;
    }
}

pub const CANDIDATE_WORDS: usize = CANDIDATE_CAPACITY.div_ceil(64);
#[derive(Clone, Copy, Debug)]
pub struct CandidatesView<'a> {
    pub action: &'a [ActionKind],
    pub model: &'a [ContentId],
    pub detail: &'a [[ContentId; ID_LANES]],
    pub source: &'a [RowHandle],
    pub target: &'a [RowHandle],
    pub actor: &'a [u8],
    pub flags: &'a [u64],
    pub flags_present: &'a [u64],
    pub integers: &'a [[i32; NUMERIC_LANES]],
    pub floats: &'a [[f32; NUMERIC_LANES]],
    pub integer_present: &'a [u16],
    pub float_present: &'a [u16],
    /// The final word's bits beyond action.len() are zero.
    pub candidate_present: &'a [u64],
    pub legal: &'a [u64],
    /// Handles into ObservationView::legacy.rows, not the V2 row array.
    pub legacy_source: Option<&'a [RowHandle]>,
    pub legacy_target: Option<&'a [RowHandle]>,
}
impl CandidatesView<'_> {
    #[inline]
    pub fn sampleable(&self, index: usize, candidate_len: usize) -> bool {
        index < candidate_len && index < self.action.len()
            && self.candidate_present[index / 64] & self.legal[index / 64] & (1 << (index % 64)) != 0
    }
}

pub struct Candidates {
    pub action: Box<[ActionKind]>,
    pub model: Box<[ContentId]>,
    pub detail: Box<[[ContentId; ID_LANES]]>,
    pub source: Box<[RowHandle]>,
    pub target: Box<[RowHandle]>,
    pub actor: Box<[u8]>,
    pub flags: Box<[u64]>,
    pub flags_present: Box<[u64]>,
    pub integers: Box<[[i32; NUMERIC_LANES]]>,
    pub floats: Box<[[f32; NUMERIC_LANES]]>,
    pub integer_present: Box<[u16]>,
    pub float_present: Box<[u16]>,
    pub candidate_present: [u64; CANDIDATE_WORDS],
    pub legal: [u64; CANDIDATE_WORDS],
}
impl Candidates {
    pub(crate) fn view(&self, len: usize) -> CandidatesView<'_> {
        let words = len.div_ceil(64);
        CandidatesView {
            action: &self.action[..len], model: &self.model[..len], detail: &self.detail[..len],
            source: &self.source[..len], target: &self.target[..len], actor: &self.actor[..len],
            flags: &self.flags[..len], flags_present: &self.flags_present[..len],
            integers: &self.integers[..len], floats: &self.floats[..len],
            integer_present: &self.integer_present[..len], float_present: &self.float_present[..len],
            candidate_present: &self.candidate_present[..words], legal: &self.legal[..words],
            legacy_source: None, legacy_target: None,
        }
    }
    pub(crate) fn new() -> Self {
        Self {
            action: vec![ActionKind::Absent; CANDIDATE_CAPACITY].into_boxed_slice(), model: vec![ContentId::ABSENT; CANDIDATE_CAPACITY].into_boxed_slice(),
            detail: vec![[ContentId::ABSENT; ID_LANES]; CANDIDATE_CAPACITY].into_boxed_slice(),
            source: vec![RowHandle::ABSENT; CANDIDATE_CAPACITY].into_boxed_slice(), target: vec![RowHandle::ABSENT; CANDIDATE_CAPACITY].into_boxed_slice(),
            actor: vec![0; CANDIDATE_CAPACITY].into_boxed_slice(), flags: vec![0; CANDIDATE_CAPACITY].into_boxed_slice(), flags_present: vec![0; CANDIDATE_CAPACITY].into_boxed_slice(),
            integers: vec![[0; NUMERIC_LANES]; CANDIDATE_CAPACITY].into_boxed_slice(), floats: vec![[0.; NUMERIC_LANES]; CANDIDATE_CAPACITY].into_boxed_slice(),
            integer_present: vec![0; CANDIDATE_CAPACITY].into_boxed_slice(), float_present: vec![0; CANDIDATE_CAPACITY].into_boxed_slice(),
            candidate_present: [0; CANDIDATE_WORDS], legal: [0; CANDIDATE_WORDS],
        }
    }
    pub(crate) fn put(&mut self, index: usize, candidate: Candidate) {
        self.action[index] = candidate.action; self.model[index] = candidate.model; self.detail[index] = candidate.detail;
        self.source[index] = candidate.source; self.target[index] = candidate.target; self.actor[index] = candidate.actor;
        self.flags[index] = candidate.flags.value; self.flags_present[index] = candidate.flags.present;
        self.integers[index] = candidate.numbers.integers; self.floats[index] = candidate.numbers.floats;
        self.integer_present[index] = candidate.numbers.integer_present; self.float_present[index] = candidate.numbers.float_present;
        let (word, bit) = (index / 64, 1u64 << (index % 64));
        self.candidate_present[word] = (self.candidate_present[word] & !bit) | (u64::from(candidate.present) * bit);
        self.legal[word] = (self.legal[word] & !bit) | (u64::from(candidate.legal) * bit);
    }
    #[inline]
    pub fn sampleable(&self, index: usize, candidate_len: usize) -> bool {
        self.view(candidate_len.min(CANDIDATE_CAPACITY)).sampleable(index, candidate_len)
    }
}
