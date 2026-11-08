//! The r7 wire of N decisions, field by field (SimInstance::write_obs_and_mask
//! order and per-decision lengths), plus raw per-lane slot access for writers
//! that fill disjoint lanes from worker threads.

pub mod vocab;
pub mod policy_rng;

pub const STATE_ROWS: usize = 512;
pub const CANDIDATES: usize = 256;

/// r7 wire rows of `n` decisions.
#[derive(Clone, Default)]
pub struct WireBatch {
    pub n: usize,
    pub state_kind: Vec<u16>,
    pub state_tokens: Vec<u16>,
    pub state_values: Vec<i32>,
    pub state_scales: Vec<f32>,
    pub state_tail: Vec<f32>,
    pub state_mask: Vec<i8>,
    pub boss: Vec<f32>,
    pub cand_kind: Vec<u16>,
    pub cand_tokens: Vec<u16>,
    pub cand_values: Vec<i32>,
    pub cand_scales: Vec<f32>,
    pub cand_tail: Vec<f32>,
    pub choice: Vec<u16>,
    pub model_mask: Vec<i8>,
    pub source_applicable: Vec<i8>,
    pub target_applicable: Vec<i8>,
    pub source_known: Vec<i8>,
    pub target_known: Vec<i8>,
    pub source_bits: Vec<u8>,
    pub target_bits: Vec<u8>,
    pub legal: Vec<bool>,
}

impl WireBatch {
    pub fn zeroed(n: usize) -> Self {
        Self {
            n,
            state_kind: vec![0; n * 512],
            state_tokens: vec![0; n * 3072],
            state_values: vec![0; n * 4096],
            state_scales: vec![0.0; n * 4096],
            state_tail: vec![0.0; n * 8192],
            state_mask: vec![0; n * 512],
            boss: vec![0.0; n * 6],
            cand_kind: vec![0; n * 256],
            cand_tokens: vec![0; n * 1536],
            cand_values: vec![0; n * 2048],
            cand_scales: vec![0.0; n * 2048],
            cand_tail: vec![0.0; n * 4096],
            choice: vec![0; n * 256],
            model_mask: vec![0; n * 256],
            source_applicable: vec![0; n * 256],
            target_applicable: vec![0; n * 256],
            source_known: vec![0; n * 256],
            target_known: vec![0; n * 256],
            source_bits: vec![0; n * 16384],
            target_bits: vec![0; n * 16384],
            legal: vec![false; n * 256],
        }
    }

    /// Raw lane slots for disjoint per-lane writes from worker threads.
    pub fn slots(&mut self) -> WireSlots {
        WireSlots {
            state_kind: Slot::new(&mut self.state_kind, 512),
            state_tokens: Slot::new(&mut self.state_tokens, 3072),
            state_values: Slot::new(&mut self.state_values, 4096),
            state_scales: Slot::new(&mut self.state_scales, 4096),
            state_tail: Slot::new(&mut self.state_tail, 8192),
            state_mask: Slot::new(&mut self.state_mask, 512),
            boss: Slot::new(&mut self.boss, 6),
            cand_kind: Slot::new(&mut self.cand_kind, 256),
            cand_tokens: Slot::new(&mut self.cand_tokens, 1536),
            cand_values: Slot::new(&mut self.cand_values, 2048),
            cand_scales: Slot::new(&mut self.cand_scales, 2048),
            cand_tail: Slot::new(&mut self.cand_tail, 4096),
            choice: Slot::new(&mut self.choice, 256),
            model_mask: Slot::new(&mut self.model_mask, 256),
            source_applicable: Slot::new(&mut self.source_applicable, 256),
            target_applicable: Slot::new(&mut self.target_applicable, 256),
            source_known: Slot::new(&mut self.source_known, 256),
            target_known: Slot::new(&mut self.target_known, 256),
            source_bits: Slot::new(&mut self.source_bits, 16384),
            target_bits: Slot::new(&mut self.target_bits, 16384),
            legal: Slot::new(&mut self.legal, 256),
        }
    }
}

/// Base pointer + per-lane length of one field; lanes are disjoint slices.
#[derive(Clone, Copy)]
pub struct Slot<T>(*mut T, usize);
unsafe impl<T> Send for Slot<T> {}
unsafe impl<T> Sync for Slot<T> {}

impl<T> Slot<T> {
    pub fn new(v: &mut [T], len: usize) -> Self {
        Self(v.as_mut_ptr(), len)
    }
    /// # Safety
    /// The backing buffer outlives the slot, `i` is in range, and no two
    /// threads write the same lane at once.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn lane(&self, i: usize) -> &mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.0.add(i * self.1), self.1) }
    }
}

#[derive(Clone, Copy)]
pub struct WireSlots {
    pub state_kind: Slot<u16>,
    pub state_tokens: Slot<u16>,
    pub state_values: Slot<i32>,
    pub state_scales: Slot<f32>,
    pub state_tail: Slot<f32>,
    pub state_mask: Slot<i8>,
    pub boss: Slot<f32>,
    pub cand_kind: Slot<u16>,
    pub cand_tokens: Slot<u16>,
    pub cand_values: Slot<i32>,
    pub cand_scales: Slot<f32>,
    pub cand_tail: Slot<f32>,
    pub choice: Slot<u16>,
    pub model_mask: Slot<i8>,
    pub source_applicable: Slot<i8>,
    pub target_applicable: Slot<i8>,
    pub source_known: Slot<i8>,
    pub target_known: Slot<i8>,
    pub source_bits: Slot<u8>,
    pub target_bits: Slot<u8>,
    pub legal: Slot<bool>,
}
