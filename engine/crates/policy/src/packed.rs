//! Packed (sparse) r7 policy forward + r8 MaskablePPO loss in burn.
//!
//! Only real work is laid out: valid state rows (state_mask) and model-mask
//! candidates become rows of dense [N, F] tensors; context means and route means
//! are `select_assign(.., Add)` segment sums; the per-decision log-softmax runs
//! on a [B, K] gather of each decision's candidates (K = most candidates in the
//! batch, padding filled with the r7 masked logit). Masked r7 candidates have
//! probability exactly 0, so this is the r7 distribution with fewer FLOPs.
use burn::prelude::*;
use burn::tensor::activation::{gelu, log_softmax, tanh};
use burn::tensor::{IndexingUpdateOp, TensorData};

use crate::model::R7Policy;
use crate::{BOSSES, CANDIDATES, MASKED_LOGIT, NUMERIC_DIM, SCALAR_SLOTS, SCALAR_WIDTH, STATE_ROWS, TAIL_SLOTS, TOKEN_SLOTS};

pub use sts2_wire::WireBatch;

/// gpu_observation_encoding.encode_raw_observation numeric channels (f64 math, f32 out).
fn numeric_row(values: &[i32], scales: &[f32], tail: &[f32], out: &mut Vec<f32>) {
    for q in 0..SCALAR_SLOTS {
        let s = scales[q] as f64;
        if s == 0.0 {
            out.extend_from_slice(&[0.0; SCALAR_WIDTH]);
            continue;
        }
        let v = values[q] as f64;
        let safe = s.max(1.0);
        let mag = v.abs();
        let sign = if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 };
        out.extend_from_slice(&[
            1.0,
            (v < 0.0) as i32 as f32,
            ((mag / 65536.0).floor() / 32768.0) as f32,
            ((mag % 65536.0) / 65535.0) as f32,
            (sign * mag.ln_1p() / safe.ln_1p()) as f32,
            (v / safe).asinh() as f32,
        ]);
    }
    out.extend_from_slice(&tail[..TAIL_SLOTS]);
}

/// Host packing of one batch, validated like _route_aggregate (fail closed).
#[derive(Clone, Default)]
pub struct PackedHost {
    pub n_dec: usize,
    pub s_kind: Vec<i32>,
    pub s_tok: Vec<i32>,
    pub s_num: Vec<f32>,
    pub s_dec: Vec<i32>,
    pub s_global: Vec<f32>,
    pub s_inv: Vec<f32>,
    pub c_kind: Vec<i32>,
    pub c_tok: Vec<i32>,
    pub c_num: Vec<f32>,
    pub c_dec: Vec<i32>,
    pub c_slot: Vec<i32>,
    pub choice: Vec<i32>,
    pub flags: Vec<f32>,
    pub link_c: [Vec<i32>; 2],
    pub link_s: [Vec<i32>; 2],
    pub link_w: [Vec<f32>; 2],
    pub boss: Vec<f32>,
    /// [B * K] candidate index per (decision, position); C (a dummy) for padding.
    pub gather: Vec<i32>,
    pub gather_valid: Vec<bool>,
    pub k: usize,
}

/// kind id -> state scalar slot zeroed (relational_observation._ROUTING_SCALAR_SLOTS).
const ROUTING: [(u16, usize); 5] = [(4, 0), (10, 0), (11, 0), (8, 4), (7, 1)];
const GLOBAL_KIND: u16 = 2;

/// Pack every decision of a wire batch.
pub fn pack(w: &WireBatch) -> Result<PackedHost, String> {
    let parts = (0..w.n).map(|b| pack_one(w, b)).collect::<Result<Vec<_>, _>>()?;
    Ok(merge(&parts.iter().collect::<Vec<_>>()))
}

/// Pack decision `lane` of a wire batch on its own (a one-decision PackedHost).
pub fn pack_one(w: &WireBatch, lane: usize) -> Result<PackedHost, String> {
    let (r, c) = (STATE_ROWS, CANDIDATES);
    let mut p = PackedHost { n_dec: 1, ..Default::default() };
    let mut state_index = vec![-1i32; r];
    let mut cand_count = vec![0usize; 1];
    for (b, src) in [(0usize, lane)] {
        let mut rows = 0usize;
        for row in 0..r {
            let i = src * r + row;
            if w.state_mask[i] == 0 {
                continue;
            }
            rows += 1;
            state_index[row] = p.s_dec.len() as i32;
            let kind = w.state_kind[i];
            p.s_kind.push(kind as i32);
            p.s_tok.extend(w.state_tokens[i * TOKEN_SLOTS..(i + 1) * TOKEN_SLOTS].iter().map(|&t| t as i32));
            let start = p.s_num.len();
            numeric_row(&w.state_values[i * 8..(i + 1) * 8], &w.state_scales[i * 8..(i + 1) * 8],
                &w.state_tail[i * 16..(i + 1) * 16], &mut p.s_num);
            for &(k, slot) in &ROUTING {
                if kind == k {
                    p.s_num[start + slot * SCALAR_WIDTH..start + (slot + 1) * SCALAR_WIDTH].fill(0.0);
                }
            }
            p.s_dec.push(b as i32);
            p.s_global.push((kind == GLOBAL_KIND) as i32 as f32);
        }
        p.s_inv.push(1.0 / rows.max(1) as f32);
        p.boss.extend_from_slice(&w.boss[src * BOSSES..(src + 1) * BOSSES]);
    }
    for (b, count) in cand_count.iter_mut().enumerate() {
        for cand in 0..c {
            let i = lane * c + cand;
            if w.model_mask[i] == 0 {
                continue;
            }
            *count += 1;
            let ci = p.c_dec.len() as i32;
            p.c_kind.push(w.cand_kind[i] as i32);
            p.c_tok.extend(w.cand_tokens[i * TOKEN_SLOTS..(i + 1) * TOKEN_SLOTS].iter().map(|&t| t as i32));
            numeric_row(&w.cand_values[i * 8..(i + 1) * 8], &w.cand_scales[i * 8..(i + 1) * 8],
                &w.cand_tail[i * 16..(i + 1) * 16], &mut p.c_num);
            p.c_dec.push(b as i32);
            p.c_slot.push(cand as i32);
            p.choice.push(w.choice[i] as i32);
            let sides = [(&w.source_bits, &w.source_applicable, &w.source_known), (&w.target_bits, &w.target_applicable, &w.target_known)];
            for (side, (bits, app, known)) in sides.into_iter().enumerate() {
                let row_bits = &bits[i * 64..(i + 1) * 64];
                let mut links = Vec::new();
                for (byte, &v) in row_bits.iter().enumerate() {
                    for bit in 0..8 {
                        if (v >> bit) & 1 != 0 {
                            let s = byte * 8 + bit;
                            let si = state_index[s];
                            if si < 0 {
                                return Err("link points to padded/out-of-range state".into());
                            }
                            links.push(si);
                        }
                    }
                }
                if app[i] == 0 && !links.is_empty() {
                    return Err("non-applicable route is not empty".into());
                }
                if app[i] != 0 && (known[i] == 0 || links.is_empty()) {
                    return Err("model-eligible candidate has unresolved route".into());
                }
                let wgt = 1.0 / links.len().max(1) as f32;
                for si in links {
                    p.link_c[side].push(ci);
                    p.link_s[side].push(si);
                    p.link_w[side].push(wgt);
                }
            }
            p.flags.extend_from_slice(&[w.source_applicable[i] as f32, w.target_applicable[i] as f32,
                w.source_known[i] as f32, w.target_known[i] as f32]);
        }
        if *count == 0 {
            return Err("refusing to build a PPO distribution from an all-false mask".into());
        }
    }
    let k = cand_count.iter().copied().max().unwrap_or(1);
    let n_cand = p.c_dec.len() as i32;
    let mut first = 0usize;
    for &count in &cand_count {
        for j in 0..k {
            p.gather.push(if j < count { (first + j) as i32 } else { n_cand });
            p.gather_valid.push(j < count);
        }
        first += count;
    }
    p.k = k;
    Ok(p)
}

/// Concatenate packed decisions into one batch (re-indexing decisions, state rows
/// and candidates) and rebuild the [B, K] candidate gather.
pub fn merge(parts: &[&PackedHost]) -> PackedHost {
    let mut p = PackedHost::default();
    let mut counts = Vec::new();
    for part in parts {
        let (d0, s0, c0) = (p.n_dec as i32, p.s_dec.len() as i32, p.c_dec.len() as i32);
        p.n_dec += part.n_dec;
        p.s_kind.extend_from_slice(&part.s_kind);
        p.s_tok.extend_from_slice(&part.s_tok);
        p.s_num.extend_from_slice(&part.s_num);
        p.s_dec.extend(part.s_dec.iter().map(|&d| d + d0));
        p.s_global.extend_from_slice(&part.s_global);
        p.s_inv.extend_from_slice(&part.s_inv);
        p.c_kind.extend_from_slice(&part.c_kind);
        p.c_tok.extend_from_slice(&part.c_tok);
        p.c_num.extend_from_slice(&part.c_num);
        p.c_dec.extend(part.c_dec.iter().map(|&d| d + d0));
        p.c_slot.extend_from_slice(&part.c_slot);
        p.choice.extend_from_slice(&part.choice);
        p.flags.extend_from_slice(&part.flags);
        for side in 0..2 {
            p.link_c[side].extend(part.link_c[side].iter().map(|&c| c + c0));
            p.link_s[side].extend(part.link_s[side].iter().map(|&s| s + s0));
            p.link_w[side].extend_from_slice(&part.link_w[side]);
        }
        p.boss.extend_from_slice(&part.boss);
        let mut per = vec![0usize; part.n_dec];
        for &d in &part.c_dec {
            per[d as usize] += 1;
        }
        counts.extend(per);
    }
    let k = counts.iter().copied().max().unwrap_or(1).max(1);
    let n_cand = p.c_dec.len() as i32;
    let mut first = 0usize;
    for &count in &counts {
        for j in 0..k {
            p.gather.push(if j < count { (first + j) as i32 } else { n_cand });
            p.gather_valid.push(j < count);
        }
        first += count;
    }
    p.k = k;
    p
}

/// Position of each decision's chosen action slot within its packed candidates.
pub fn action_positions(p: &PackedHost, actions: &[i32]) -> Result<Vec<i32>, String> {
    let mut pos = vec![-1i32; p.n_dec];
    let mut seen = vec![0i32; p.n_dec];
    for (&b, &slot) in p.c_dec.iter().zip(&p.c_slot) {
        let b = b as usize;
        if slot == actions[b] {
            pos[b] = seen[b];
        }
        seen[b] += 1;
    }
    match pos.iter().position(|&x| x < 0) {
        Some(b) => Err(format!("action {} of decision {b} is not model-legal", actions[b])),
        None => Ok(pos),
    }
}

/// Device tensors of one packed batch.
pub struct PackedBatch {
    pub b: usize,
    pub k: usize,
    pub s_kind: Tensor<1, Int>,
    pub s_tok: Tensor<2, Int>,
    pub s_tok_present: Tensor<3>,
    pub s_num: Tensor<2>,
    pub s_dec: Tensor<1, Int>,
    pub s_global: Tensor<2>,
    pub s_inv: Tensor<2>,
    pub c_kind: Tensor<1, Int>,
    pub c_tok: Tensor<2, Int>,
    pub c_tok_present: Tensor<3>,
    pub c_num: Tensor<2>,
    pub c_dec: Tensor<1, Int>,
    pub choice: Tensor<1, Int>,
    pub flags: Tensor<2>,
    pub links: [(Tensor<1, Int>, Tensor<1, Int>, Tensor<2>); 2],
    pub boss: Tensor<2>,
    pub gather: Tensor<1, Int>,
    pub gather_invalid: Tensor<2, Bool>,
}

impl PackedBatch {
    pub fn new(p: &PackedHost, device: &Device) -> Self {
        let s = p.s_dec.len();
        let c = p.c_dec.len();
        let i1 = |v: &Vec<i32>| Tensor::<1, Int>::from_data(TensorData::new(v.clone(), [v.len()]), device);
        let f2 = |v: &Vec<f32>, cols: usize| Tensor::<2>::from_data(TensorData::new(v.clone(), [v.len() / cols, cols]), device);
        let present = |tok: &Vec<i32>, n: usize| Tensor::<3>::from_data(
            TensorData::new(tok.iter().map(|&t| (t != 0) as i32 as f32).collect::<Vec<_>>(), [n, TOKEN_SLOTS, 1]), device);
        let links = std::array::from_fn(|side| {
            let w = &p.link_w[side];
            (i1(&p.link_c[side]), i1(&p.link_s[side]),
             Tensor::<2>::from_data(TensorData::new(w.clone(), [w.len(), 1]), device))
        });
        Self {
            b: p.n_dec,
            k: p.k,
            s_kind: i1(&p.s_kind),
            s_tok: Tensor::<2, Int>::from_data(TensorData::new(p.s_tok.clone(), [s, TOKEN_SLOTS]), device),
            s_tok_present: present(&p.s_tok, s),
            s_num: f2(&p.s_num, NUMERIC_DIM),
            s_dec: i1(&p.s_dec),
            s_global: f2(&p.s_global, 1),
            s_inv: Tensor::<2>::from_data(TensorData::new(
                p.s_dec.iter().map(|&b| p.s_inv[b as usize]).collect::<Vec<_>>(), [s, 1]), device),
            c_kind: i1(&p.c_kind),
            c_tok: Tensor::<2, Int>::from_data(TensorData::new(p.c_tok.clone(), [c, TOKEN_SLOTS]), device),
            c_tok_present: present(&p.c_tok, c),
            c_num: f2(&p.c_num, NUMERIC_DIM),
            c_dec: i1(&p.c_dec),
            choice: i1(&p.choice),
            flags: f2(&p.flags, 4),
            links,
            boss: f2(&p.boss, BOSSES),
            gather: i1(&p.gather),
            gather_invalid: Tensor::<2, Bool>::from_data(TensorData::new(
                p.gather_valid.iter().map(|&v| !v).collect::<Vec<_>>(), [p.n_dec, p.k]), device),
        }
    }
}

/// Outputs of one packed forward (+ log-softmax over each decision's candidates).
pub struct PackedOut {
    /// [B, K] log-probabilities; padding positions hold ~-1e9.
    pub logp: Tensor<2>,
    /// [B] values.
    pub value: Tensor<1>,
}

impl R7Policy {
    fn encode_packed(&self, kind: Tensor<1, Int>, tok: Tensor<2, Int>, present: Tensor<3>, num: Tensor<2>) -> Tensor<2> {
        let [n] = kind.dims();
        let k = self.kind_embedding.forward(kind.reshape([n, 1])).reshape([n, self.kind_embedding.weight.dims()[1]]);
        // padding_idx=0: token 0 contributes nothing and receives no gradient.
        let t = (self.token_embedding.forward(tok) * present).sum_dim(1).reshape([n, self.token_embedding.weight.dims()[1]]);
        let x = Tensor::cat(vec![k, t, gelu(self.numeric_projection.forward(num))], 1);
        gelu(self.shared_encoder_1.forward(gelu(self.shared_encoder_0.forward(x))))
    }

    pub fn forward_packed(&self, p: &PackedBatch) -> PackedOut {
        let h = self.encode_packed(p.s_kind.clone(), p.s_tok.clone(), p.s_tok_present.clone(), p.s_num.clone());
        let [_, hid] = h.dims();
        let device = h.device();
        // PublicBossActor: boss delta on global rows only.
        let delta = self.boss_projection.forward(p.boss.clone()).select(0, p.s_dec.clone());
        let state = h + delta * p.s_global.clone();
        let context = Tensor::<2>::zeros([p.b, hid], &device)
            .select_assign(0, p.s_dec.clone(), state.clone() * p.s_inv.clone(), IndexingUpdateOp::Add);
        let value = self.value_1.forward(tanh(self.value_0.forward(context.clone()))).reshape([p.b]);
        let cand = self.encode_packed(p.c_kind.clone(), p.c_tok.clone(), p.c_tok_present.clone(), p.c_num.clone());
        let [c, _] = cand.dims();
        let route = |side: usize| {
            let (lc, ls, lw) = &p.links[side];
            Tensor::<2>::zeros([c, hid], &device)
                .select_assign(0, lc.clone(), state.clone().select(0, ls.clone()) * lw.clone(), IndexingUpdateOp::Add)
        };
        let (src, tgt) = (route(0), route(1));
        let choice = self.choice_embedding.forward(p.choice.clone().reshape([c, 1])).reshape([c, self.choice_embedding.weight.dims()[1]]);
        let relation = gelu(self.relation_projection.forward(
            Tensor::cat(vec![src.clone(), tgt.clone(), choice, p.flags.clone()], 1)));
        let ctx = context.select(0, p.c_dec.clone());
        let features = Tensor::cat(vec![
            cand.clone(), ctx.clone(), src.clone(), tgt.clone(),
            cand.clone() * ctx, cand.clone() * src, cand * tgt, relation,
        ], 1);
        let logit = self.candidate_head_1.forward(gelu(self.candidate_head_0.forward(features))).reshape([c]);
        // Gather each decision's candidates; padding points at an appended dummy.
        let padded = Tensor::cat(vec![logit, Tensor::<1>::zeros([1], &device)], 0)
            .select(0, p.gather.clone())
            .reshape([p.b, p.k])
            .mask_fill(p.gather_invalid.clone(), MASKED_LOGIT);
        PackedOut { logp: log_softmax(padded, 1), value }
    }
}

/// r8 MaskablePPO minibatch loss (ppo.py): clipped policy loss, MSE value loss,
/// entropy bonus. `act_pos` is the chosen candidate's position within its decision.
#[allow(clippy::too_many_arguments)]
pub fn ppo_loss(out: &PackedOut, act_pos: Tensor<2, Int>, old_logp: Tensor<1>, adv: Tensor<1>, ret: Tensor<1>,
                clip: f32, ent_coef: f32, vf_coef: f32) -> (Tensor<1>, Tensor<1>, Tensor<1>) {
    let (loss,logp,entropy,_)=ppo_loss_inner(out,act_pos,old_logp,adv,ret,clip,ent_coef,vf_coef,None,None,None);
    (loss,logp,entropy)
}

/// Fixed-shape minibatches: padding has zero loss and zero gradient.
pub fn ppo_loss_weighted(out: &PackedOut, act_pos: Tensor<2, Int>, old_logp: Tensor<1>, adv: Tensor<1>, ret: Tensor<1>,
                clip: f32, ent_coef: f32, vf_coef: f32, weights: Tensor<1>) -> (Tensor<1>, Tensor<1>, Tensor<1>) {
    let (loss,logp,entropy,_)=ppo_loss_inner(out,act_pos,old_logp,adv,ret,clip,ent_coef,vf_coef,Some(weights),None,None);
    (loss,logp,entropy)
}
/// Same objective, with the unreduced squared value error for diagnostics.
/// `entropy_scale` multiplies `ent_coef` per sample (per decision head).
pub fn ppo_loss_weighted_with_value(out:&PackedOut,act_pos:Tensor<2,Int>,old_logp:Tensor<1>,
    adv:Tensor<1>,ret:Tensor<1>,clip:f32,ent_coef:f32,vf_coef:f32,weights:Tensor<1>,entropy_scale:Tensor<1>)
    ->(Tensor<1>,Tensor<1>,Tensor<1>,Tensor<1>) {
    ppo_loss_inner(out,act_pos,old_logp,adv,ret,clip,ent_coef,vf_coef,Some(weights),None,Some(entropy_scale))
}
/// Weighted PPO contribution normalized by an enclosing logical minibatch.
/// Layout-bounded sub-chunks can therefore accumulate gradients before one
/// clip and optimizer step without changing the PPO objective.
pub fn ppo_loss_weighted_with_value_normalized(out:&PackedOut,act_pos:Tensor<2,Int>,old_logp:Tensor<1>,
    adv:Tensor<1>,ret:Tensor<1>,clip:f32,ent_coef:f32,vf_coef:f32,weights:Tensor<1>,normalization:f32,
    entropy_scale:Tensor<1>)
    ->(Tensor<1>,Tensor<1>,Tensor<1>,Tensor<1>) {
    assert!(normalization.is_finite() && normalization>0.);
    ppo_loss_inner(out,act_pos,old_logp,adv,ret,clip,ent_coef,vf_coef,Some(weights),Some(normalization),Some(entropy_scale))
}
#[allow(clippy::too_many_arguments)]
fn ppo_loss_inner(out: &PackedOut, act_pos: Tensor<2, Int>, old_logp: Tensor<1>, adv: Tensor<1>, ret: Tensor<1>,
                clip: f32, ent_coef: f32, vf_coef: f32, weights: Option<Tensor<1>>, normalization:Option<f32>,
                entropy_scale:Option<Tensor<1>>) -> (Tensor<1>, Tensor<1>, Tensor<1>, Tensor<1>) {
    let [b, _] = out.logp.dims();
    let logp = out.logp.clone().gather(1, act_pos).reshape([b]);
    // Native padding uses -inf, not a finite logit competitor. Define 0 log 0
    // as zero before multiplying, so padding contributes no entropy/gradient.
    let entropy_logp = out.logp.clone().mask_fill(out.logp.clone().equal_elem(f32::NEG_INFINITY),0.);
    let entropy = (out.logp.clone().exp() * entropy_logp).sum_dim(1).reshape([b]).neg();
    let ratio = (logp.clone() - old_logp).exp();
    let mean=|x:Tensor<1>| match &weights {
        Some(weights)=>{
            let sum=(x*weights.clone()).sum();
            match normalization {Some(n)=>sum/n,None=>sum/weights.clone().sum()}
        },
        None=>x.mean(),
    };
    let pl = mean((adv.clone() * ratio.clone()).min_pair(adv * ratio.clamp(1.0 - clip, 1.0 + clip))).neg();
    let value_error=(ret-out.value.clone()).powi_scalar(2);
    let vl = mean(value_error.clone());
    let bonus = match entropy_scale {Some(scale)=>mean(entropy.clone()*scale),None=>mean(entropy.clone())};
    let loss = pl + bonus.neg() * ent_coef + vl * vf_coef;
    (loss, logp, entropy, value_error)
}
