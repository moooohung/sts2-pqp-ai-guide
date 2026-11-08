use burn::module::{Module, Param};
use burn::nn::{Embedding, EmbeddingConfig, Linear, LinearConfig};
use burn::prelude::*;
use burn::tensor::activation::{gelu, tanh};

use crate::{BOSSES, CANDIDATES, MASKED_LOGIT, NUMERIC_DIM, TOKEN_SLOTS};

/// Sizes of the r7 checkpoint (policy_contract.json).
#[derive(Clone, Copy, Debug)]
pub struct PolicyDims {
    pub hidden: usize,
    pub token: usize,
    pub kind: usize,
    pub choice: usize,
    pub kind_vocab: usize,
    pub token_vocab: usize,
    pub choice_vocab: usize,
}

impl PolicyDims {
    pub const R7: Self = Self { hidden: 64, token: 24, kind: 12, choice: 8, kind_vocab: 22, token_vocab: 2124, choice_vocab: 13 };
    pub const R8: Self = Self { token_vocab: sts2_wire::vocab::R8_TOKEN_VOCAB, ..Self::R7 };
}

#[cfg(test)]
mod fresh_tests {
    use super::*;

    #[test]
    fn fresh_model_uses_supplied_counts_widths_and_seed() {
        let _rng_guard = crate::RNG_TEST_LOCK.lock().unwrap();
        let device=Device::flex();
        let dims=PolicyDims {hidden:8,kind:6,token:12,choice:4,
            kind_vocab:31,token_vocab:2300,choice_vocab:19};
        device.seed(20261030);
        let first=R7Policy::init(dims,&device);
        let weights=first.token_embedding.weight.val().into_data().try_to_vec::<f32>().unwrap();
        device.seed(20261030);
        let second=R7Policy::init(dims,&device);
        assert_eq!(weights,second.token_embedding.weight.val().into_data().try_to_vec::<f32>().unwrap());
        assert_eq!(first.kind_embedding.weight.dims(),[31,6]);
        assert_eq!(first.token_embedding.weight.dims(),[2300,12]);
        assert_eq!(first.choice_embedding.weight.dims(),[19,4]);
        assert_eq!(first.shared_encoder_0.weight.dims(),[26,8]);
        assert_eq!(first.relation_projection.weight.dims(),[24,8]);
    }
}

/// PublicBossActor + value head. Field names are the remapped r7 state_dict keys.
#[derive(Module, Debug)]
pub struct R7Policy {
    pub kind_embedding: Embedding,
    pub token_embedding: Embedding,
    pub numeric_projection: Linear,
    pub shared_encoder_0: Linear,
    pub shared_encoder_1: Linear,
    pub choice_embedding: Embedding,
    pub relation_projection: Linear,
    pub candidate_head_0: Linear,
    pub candidate_head_1: Linear,
    pub boss_projection: Linear,
    pub value_0: Linear,
    pub value_1: Linear,
}

/// One device-resident batch, already validated and preprocessed on the host
/// (state numeric sanitized, route links normalized by their counts).
pub struct PolicyBatch {
    pub state_kind: Tensor<2, Int>,
    pub state_tokens: Tensor<3, Int>,
    pub state_numeric: Tensor<3>,
    pub state_mask: Tensor<2>,
    pub global_rows: Tensor<2>,
    pub cand_kind: Tensor<2, Int>,
    pub cand_tokens: Tensor<3, Int>,
    pub cand_numeric: Tensor<3>,
    pub choice: Tensor<2, Int>,
    pub model_mask: Tensor<2, Bool>,
    pub source_weights: Tensor<3>,
    pub target_weights: Tensor<3>,
    pub flags: Tensor<3>,
    pub boss: Tensor<2>,
}

impl R7Policy {
    /// r8 vocabulary v2: SPECIAL_CARD copies pick_card (1423); optional
    /// discard-to-hand copies choose (1442). Preserve IDs and
    /// every old weight bit; the detached result is a fresh optimizer leaf.
    pub fn with_r8_vocab(mut self) -> Self {
        use sts2_wire::vocab::*;
        let old = self.token_embedding.weight.val();
        let [rows, width] = old.dims();
        if rows == R8_TOKEN_VOCAB { return self; }
        assert!(rows == R7_TOKEN_VOCAB || rows == OPTIONAL_DISCARD_TO_HAND as usize,
            "unsupported vocabulary migration");
        let grad = old.is_require_grad();
        let mut parts = vec![old.clone()];
        if rows == R7_TOKEN_VOCAB {
            parts.push(old.clone().slice([PICK_CARD..PICK_CARD+1, 0..width]));
        }
        parts.push(old.slice([CHOOSE..CHOOSE+1, 0..width]));
        let grown = Tensor::cat(parts, 0).detach().set_require_grad(grad);
        self.token_embedding.weight = Param::initialized(self.token_embedding.weight.id, grown);
        self
    }

    pub fn init(dims: PolicyDims, device: &Device) -> Self {
        let h = dims.hidden;
        let lin = |i, o| LinearConfig::new(i, o).init(device);
        Self {
            kind_embedding: EmbeddingConfig::new(dims.kind_vocab, dims.kind).init(device),
            token_embedding: EmbeddingConfig::new(dims.token_vocab, dims.token).init(device),
            numeric_projection: lin(NUMERIC_DIM, h),
            shared_encoder_0: lin(dims.kind + dims.token + h, h),
            shared_encoder_1: lin(h, h),
            choice_embedding: EmbeddingConfig::new(dims.choice_vocab, dims.choice).init(device),
            relation_projection: lin(h * 2 + dims.choice + 4, h),
            candidate_head_0: lin(h * 8, h),
            candidate_head_1: lin(h, 1),
            boss_projection: LinearConfig::new(BOSSES, h).with_bias(false).init(device),
            value_0: lin(h, h),
            value_1: lin(h, 1),
        }
    }


    fn encode_rows(&self, kind: Tensor<2, Int>, tokens: Tensor<3, Int>, numeric: Tensor<3>) -> Tensor<3> {
        let [b, n, _] = tokens.dims();
        let k = self.kind_embedding.forward(kind);
        // padding_idx=0: token 0 contributes nothing and receives no gradient.
        let flat = tokens.reshape([b, n * TOKEN_SLOTS]);
        let present = flat.clone().not_equal_elem(0).float().unsqueeze_dim::<3>(2);
        let t = (self.token_embedding.forward(flat) * present)
            .reshape([b, n, TOKEN_SLOTS, self.token_embedding.weight.dims()[1]])
            .sum_dim(2)
            .squeeze_dim::<3>(2);
        let num = gelu(self.numeric_projection.forward(numeric));
        let x = Tensor::cat(vec![k, t, num], 2);
        gelu(self.shared_encoder_1.forward(gelu(self.shared_encoder_0.forward(x))))
    }

    /// (state rows, mean-pooled context) for a batch.
    pub fn state(&self, batch: &PolicyBatch) -> (Tensor<3>, Tensor<2>) {
        let mut state = self.encode_rows(batch.state_kind.clone(), batch.state_tokens.clone(), batch.state_numeric.clone());
        // PublicBossActor: delta on the global rows (zero feature -> zero delta, the legacy path).
        let delta = self.boss_projection.forward(batch.boss.clone()).unsqueeze_dim::<3>(1);
        state = state + delta * batch.global_rows.clone().unsqueeze_dim::<3>(2);
        let mask = batch.state_mask.clone().unsqueeze_dim::<3>(2);
        let denom = mask.clone().sum_dim(1).clamp_min(1.0);
        let context = ((state.clone() * mask).sum_dim(1) / denom).squeeze_dim::<2>(1);
        (state, context)
    }

    pub fn value_from_context(&self, context: Tensor<2>) -> Tensor<1> {
        let [b, _] = context.dims();
        self.value_1.forward(tanh(self.value_0.forward(context))).reshape([b])
    }

    /// Masked candidate logits [B, 256] and values [B].
    pub fn forward(&self, batch: &PolicyBatch) -> (Tensor<2>, Tensor<1>) {
        let (state, context) = self.state(batch);
        let [b, _, h] = state.dims();
        let source = batch.source_weights.clone().matmul(state.clone());
        let target = batch.target_weights.clone().matmul(state);
        let cands = self.encode_rows(batch.cand_kind.clone(), batch.cand_tokens.clone(), batch.cand_numeric.clone());
        let expanded = context.clone().unsqueeze_dim::<3>(1).expand([b, CANDIDATES, h]);
        let choice = self.choice_embedding.forward(batch.choice.clone());
        let relation = gelu(self.relation_projection.forward(Tensor::cat(
            vec![source.clone(), target.clone(), choice, batch.flags.clone()], 2)));
        let features = Tensor::cat(vec![
            cands.clone(), expanded.clone(), source.clone(), target.clone(),
            cands.clone() * expanded, cands.clone() * source, cands * target, relation,
        ], 2);
        let logits = self.candidate_head_1.forward(gelu(self.candidate_head_0.forward(features)))
            .reshape([b, CANDIDATES])
            .mask_fill(batch.model_mask.clone().bool_not(), MASKED_LOGIT);
        (logits, self.value_from_context(context))
    }
}
