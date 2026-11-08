//! Host-side observation preparation, exactly as the r7 policy consumes it.
//! Wire numeric channels follow observation212.scalar_channel212 / encode_rows212;
//! sanitizing and route checks follow relational_structured_ppo.py.
use std::collections::HashMap;

use burn::prelude::*;
use burn::tensor::TensorData;

use crate::model::PolicyBatch;
use crate::{BOSSES, CANDIDATES, NUMERIC_DIM, SCALAR_SLOTS, SCALAR_WIDTH, STATE_ROWS, TAIL_SLOTS, TOKEN_SLOTS};

/// One decision's policy observation (unpacked, row-major).
#[derive(Clone)]
pub struct HostObs {
    pub state_kind: Vec<u16>,       // [512]
    pub state_tokens: Vec<u16>,     // [512 * 6]
    pub state_numeric: Vec<f32>,    // [512 * 64], unsanitized
    pub state_mask: Vec<u8>,        // [512]
    pub cand_kind: Vec<u16>,        // [256]
    pub cand_tokens: Vec<u16>,      // [256 * 6]
    pub cand_numeric: Vec<f32>,     // [256 * 64]
    pub choice: Vec<u16>,           // [256]
    pub model_mask: Vec<u8>,        // [256]
    pub source_applicable: Vec<u8>, // [256]
    pub source_known: Vec<u8>,
    pub target_applicable: Vec<u8>,
    pub target_known: Vec<u8>,
    pub source_links: Vec<u8>,      // [256 * 512] 0/1
    pub target_links: Vec<u8>,
    pub boss: Vec<f32>,             // [6]
}

/// observation212.scalar_channel212: one integer scalar with its f64 scale -> 6 f32 channels.
pub fn scalar_channels(value: i32, scale: f64) -> [f32; SCALAR_WIDTH] {
    if scale == 0.0 {
        return [0.0; SCALAR_WIDTH];
    }
    let magnitude = (value as i64).abs();
    let safe = scale.max(1.0);
    let sign = if value < 0 { -1.0 } else if value > 0 { 1.0 } else { 0.0 };
    [
        1.0,
        (value < 0) as i32 as f32,
        ((magnitude >> 16) as f64 / 32768.0) as f32,
        ((magnitude & 65535) as f64 / 65535.0) as f32,
        (sign * (1.0 + magnitude as f64).ln() / (1.0 + safe).ln()) as f32,
        asinh212(value as f64 / safe) as f32,
    ]
}

/// observation212.asinh212 (sign-preserving log form, not libm asinh).
pub fn asinh212(x: f64) -> f64 {
    let ax = x.abs();
    let r = (ax + (ax * ax + 1.0).sqrt()).ln();
    if x < 0.0 { -r } else { r }
}

/// Wire row -> 64 policy numeric features (48 scalar channels + 16 tail values).
/// `scales` are the f32 wire scales; validate_observation212 guarantees they are exact.
pub fn numeric_row(values: &[i32], scales: &[f32], tail: &[f32], out: &mut [f32]) {
    for q in 0..SCALAR_SLOTS {
        let ch = scalar_channels(values[q], scales[q] as f64);
        out[q * SCALAR_WIDTH..(q + 1) * SCALAR_WIDTH].copy_from_slice(&ch);
    }
    out[SCALAR_SLOTS * SCALAR_WIDTH..NUMERIC_DIM].copy_from_slice(&tail[..TAIL_SLOTS]);
}

#[derive(Clone, Debug)]
pub struct ObsConfig {
    pub global_kind: u16,
    /// kind id -> scalar slots zeroed in state rows (relational_observation._ROUTING_SCALAR_SLOTS).
    pub routing_scalar_slots: HashMap<u16, Vec<usize>>,
}

impl ObsConfig {
    pub fn from_contract(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let global_kind = v["global_kind_id"].as_u64().ok_or("global_kind_id")? as u16;
        let mut routing_scalar_slots = HashMap::new();
        for (k, slots) in v["routing_scalar_slots"].as_object().ok_or("routing_scalar_slots")? {
            let slots = slots.as_array().ok_or("slot list")?.iter().map(|s| s.as_u64().unwrap() as usize).collect();
            routing_scalar_slots.insert(k.parse::<u16>().map_err(|e| e.to_string())?, slots);
        }
        Ok(Self { global_kind, routing_scalar_slots })
    }
}

/// Validate (fail closed, as RelationalStructuredCandidateActor) and stack a batch.
pub fn build_batch(obs: &[HostObs], cfg: &ObsConfig, device: &Device) -> Result<PolicyBatch, String> {
    let b = obs.len();
    let (r, c) = (STATE_ROWS, CANDIDATES);
    let mut state_kind = Vec::with_capacity(b * r);
    let mut state_tokens = Vec::with_capacity(b * r * TOKEN_SLOTS);
    let mut state_numeric = Vec::with_capacity(b * r * NUMERIC_DIM);
    let mut state_mask = Vec::with_capacity(b * r);
    let mut global_rows = Vec::with_capacity(b * r);
    let mut cand_kind = Vec::with_capacity(b * c);
    let mut cand_tokens = Vec::with_capacity(b * c * TOKEN_SLOTS);
    let mut cand_numeric = Vec::with_capacity(b * c * NUMERIC_DIM);
    let mut choice = Vec::with_capacity(b * c);
    let mut model_mask = Vec::with_capacity(b * c);
    let mut source_w = vec![0f32; b * c * r];
    let mut target_w = vec![0f32; b * c * r];
    let mut flags = Vec::with_capacity(b * c * 4);
    let mut boss = Vec::with_capacity(b * BOSSES);
    for (i, o) in obs.iter().enumerate() {
        if !o.model_mask.iter().any(|&m| m != 0) {
            return Err("refusing to build a PPO distribution from an all-false mask".into());
        }
        for row in 0..r {
            let kind = o.state_kind[row];
            state_kind.push(kind as i32);
            state_mask.push(o.state_mask[row] as f32);
            global_rows.push((kind == cfg.global_kind) as i32 as f32);
            let mut num = o.state_numeric[row * NUMERIC_DIM..(row + 1) * NUMERIC_DIM].to_vec();
            if let Some(slots) = cfg.routing_scalar_slots.get(&kind) {
                for &s in slots {
                    num[s * SCALAR_WIDTH..(s + 1) * SCALAR_WIDTH].fill(0.0);
                }
            }
            state_numeric.extend_from_slice(&num);
        }
        state_tokens.extend(o.state_tokens.iter().map(|&t| t as i32));
        cand_kind.extend(o.cand_kind.iter().map(|&k| k as i32));
        cand_tokens.extend(o.cand_tokens.iter().map(|&t| t as i32));
        cand_numeric.extend_from_slice(&o.cand_numeric);
        choice.extend(o.choice.iter().map(|&x| x as i32));
        model_mask.extend(o.model_mask.iter().map(|&m| m != 0));
        for (side, links, applicable, known, out) in [
            ("source", &o.source_links, &o.source_applicable, &o.source_known, &mut source_w),
            ("target", &o.target_links, &o.target_applicable, &o.target_known, &mut target_w),
        ] {
            for cand in 0..c {
                let row = &links[cand * r..(cand + 1) * r];
                let mut count = 0usize;
                for (s, &l) in row.iter().enumerate() {
                    if l != 0 {
                        if o.state_mask[s] == 0 {
                            return Err(format!("{side} link points to padded/out-of-range state"));
                        }
                        count += 1;
                    }
                }
                if applicable[cand] == 0 && count != 0 {
                    return Err(format!("non-applicable {side} route is not empty"));
                }
                if o.model_mask[cand] != 0 && applicable[cand] != 0 && (known[cand] == 0 || count == 0) {
                    return Err(format!("model-eligible candidate has unresolved {side} route"));
                }
                let w = 1.0 / count.max(1) as f32;
                let base = (i * c + cand) * r;
                for (s, &l) in row.iter().enumerate() {
                    if l != 0 {
                        out[base + s] = w;
                    }
                }
            }
        }
        for cand in 0..c {
            flags.extend_from_slice(&[o.source_applicable[cand] as f32, o.target_applicable[cand] as f32,
                o.source_known[cand] as f32, o.target_known[cand] as f32]);
        }
        boss.extend_from_slice(&o.boss);
    }
    let f3 = |v: Vec<f32>, d: [usize; 3]| Tensor::<3>::from_data(TensorData::new(v, d), device);
    let i3 = |v: Vec<i32>, d: [usize; 3]| Tensor::<3, Int>::from_data(TensorData::new(v, d), device);
    Ok(PolicyBatch {
        state_kind: Tensor::from_data(TensorData::new(state_kind, [b, r]), device),
        state_tokens: i3(state_tokens, [b, r, TOKEN_SLOTS]),
        state_numeric: f3(state_numeric, [b, r, NUMERIC_DIM]),
        state_mask: Tensor::from_data(TensorData::new(state_mask, [b, r]), device),
        global_rows: Tensor::from_data(TensorData::new(global_rows, [b, r]), device),
        cand_kind: Tensor::from_data(TensorData::new(cand_kind, [b, c]), device),
        cand_tokens: i3(cand_tokens, [b, c, TOKEN_SLOTS]),
        cand_numeric: f3(cand_numeric, [b, c, NUMERIC_DIM]),
        choice: Tensor::from_data(TensorData::new(choice, [b, c]), device),
        model_mask: Tensor::from_data(TensorData::new(model_mask, [b, c]), device),
        source_weights: f3(source_w, [b, c, r]),
        target_weights: f3(target_w, [b, c, r]),
        flags: f3(flags, [b, c, 4]),
        boss: Tensor::from_data(TensorData::new(boss, [b, BOSSES]), device),
    })
}
