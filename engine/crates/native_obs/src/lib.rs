//! NativeObsV2: generated content identities, policy schema, and caller-owned buffers.
//! Read-only phase encoders and authenticated, generation-checked dispatch.
#![forbid(unsafe_code)]
mod binding;
mod buffer;
mod layout;
mod encoder;
mod summary;
mod pile_stream;
pub mod content_hash;
pub mod live;
pub mod replay;
pub mod reward;
pub mod sampling;
pub use binding::*;
pub use buffer::*;
pub use layout::*;
pub use encoder::{DispatchError, EncodeError, NativeEncoder, TeamObsV2, TeamAction, TeamBinding, TEAM_CANDIDATE_CAPACITY};
pub use summary::SummaryGroup;
pub use pile_stream::{PileGroupSpan, StreamedPilesView, PILE_LOCAL_BASE};

/// Predicate-legal action whose original-DLL execution throws. Policy admission
/// excludes it; original legal-action audits must still retain it.
pub fn original_fault_candidate(view: &ObservationView<'_>, index: usize) -> bool {
    let candidates = view.candidates;
    if candidates.action[index] != ActionKind::PlayCard { return false; }
    let Some(target) = candidates.target[index].index() else { return false; };
    if (KnownFlags { value: view.rows.flags[target], present: view.rows.flags_present[target] })
        .get(Flag::Pet) != Some(true) { return false; }
    sts2_sim::tables::ORIGINAL_PET_TARGET_FAULTS.iter().any(|card|
        ContentId::from_index(Namespace::CardId, card.index()) == Some(candidates.model[index]))
}

#[derive(Clone, Copy, Debug)]
pub struct Registry {
    pub namespace: Namespace,
    pub name: &'static str,
    pub names: &'static [&'static str],
    pub count: usize,
    pub embedding_width: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericKind { Integer, Float, Number }
#[derive(Clone, Copy, Debug)]
pub struct NumericField { pub name: &'static str, pub kind: NumericKind }

include!(concat!(env!("OUT_DIR"), "/manifest.rs"));
pub const CONTENT_MANIFEST_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/content_manifest.json"));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContentId { pub namespace: Namespace, pub value: u16 }
impl ContentId {
    pub const ABSENT: Self = Self { namespace: Namespace::Absent, value: 0 };
    /// This is an ordinal cast and a static-array bounds check, not a name lookup.
    #[inline]
    pub fn from_index(namespace: Namespace, index: usize) -> Option<Self> {
        let registry = namespace.registry()?;
        (index < registry.count).then(|| Self { namespace, value: (index + 1) as u16 })
    }
    #[inline]
    pub fn is_valid(self) -> bool {
        if self.value == 0 { return self.namespace == Namespace::Absent; }
        self.namespace.registry().is_some_and(|r| usize::from(self.value) <= r.count)
    }
}
impl Namespace {
    #[inline]
    pub fn registry(self) -> Option<&'static Registry> {
        (self as usize).checked_sub(1).and_then(|index| REGISTRIES.get(index))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestIdentity {
    pub version: u32,
    pub schema: [u8; 32],
    pub content: [u8; 32],
}
pub const MANIFEST_IDENTITY: ManifestIdentity = ManifestIdentity {
    version: SCHEMA_VERSION, schema: SCHEMA_HASH, content: CONTENT_HASH,
};
pub const TEAM_MANIFEST_IDENTITY: ManifestIdentity = ManifestIdentity {
    version: 2, schema: TEAM_SCHEMA_HASH, content: CONTENT_HASH,
};
impl ManifestIdentity {
    pub fn validate(self) -> Result<(), ContractError> {
        if (self.version != SCHEMA_VERSION || self.schema != SCHEMA_HASH)
            && (self.version != 2 || self.schema != TEAM_SCHEMA_HASH) {
            return Err(ContractError::SchemaMismatch);
        }
        if self.content != CONTENT_HASH { return Err(ContractError::ContentMismatch); }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferKind { Rows, Candidates, Edges, Selection }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    SchemaMismatch,
    ContentMismatch,
    Capacity { buffer: BufferKind, needed: usize, capacity: usize },
    StaleBinding,
    WrongOwner,
    InvalidBinding,
    InvalidContentId,
    InvalidHandle,
    InvalidNumericField,
    NonFinite,
    InvalidFlags,
    MissingGlobal,
    EmptyActionable,
    NonActionableCandidates,
    Incomplete,
    InvalidSeat,
}
